# Gateway Attachment Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable bidirectional image/file/audio/video support between external messaging platforms (Discord) and the agent via the gateway bridge.

**Architecture:** Two new gRPC RPCs on `AgentService` (`UploadAttachment` / `GetAttachmentContent`) let the gateway store and retrieve attachment blobs without direct DB or blob access. The core upload pipeline is extracted from `sober-api` into `sober-workspace` for reuse. Gateway types gain attachment fields; Discord handler downloads from CDN inbound and sends files outbound.

**Tech Stack:** Rust, tonic/prost (gRPC), serenity (Discord), reqwest (HTTP downloads), sober-workspace (blob storage + image processing)

---

## File Structure

| File | Responsibility |
|------|---------------|
| `backend/crates/sober-workspace/src/attachment.rs` | **New.** Shared `process_and_store_attachment()` function — validate, process, blob-store, create DB record. |
| `backend/proto/sober/agent/v1/agent.proto` | Add `UploadAttachment` + `GetAttachmentContent` RPCs and messages. |
| `backend/crates/sober-agent/src/grpc/attachments.rs` | **New.** Handler functions for the two attachment RPCs. |
| `backend/crates/sober-gateway/src/types.rs` | Add `InboundAttachment`, `OutboundAttachment` structs; extend `GatewayEvent` + `PlatformMessage`. |
| `backend/crates/sober-gateway/src/error.rs` | Add attachment error variants. |
| `backend/crates/sober-gateway/src/discord/handler.rs` | Download `msg.attachments` from Discord CDN. |
| `backend/crates/sober-gateway/src/service.rs` | Upload inbound attachments via RPC, build content blocks. |
| `backend/crates/sober-gateway/src/discord/client.rs` | Send files via Serenity `CreateAttachment`. |
| `backend/crates/sober-gateway/src/helpers.rs` | Detect media in outbound `NewMessage`, fetch and deliver. |

---

### Task 1: Extract shared attachment logic into `sober-workspace`

**Files:**
- Create: `backend/crates/sober-workspace/src/attachment.rs`
- Modify: `backend/crates/sober-workspace/src/lib.rs`
- Modify: `backend/crates/sober-workspace/Cargo.toml`
- Modify: `backend/crates/sober-api/src/services/attachment.rs`

- [ ] **Step 1: Add `sober-db` and `sqlx` dependencies to `sober-workspace`**

In `backend/crates/sober-workspace/Cargo.toml`, add:
```toml
sober-db = { path = "../sober-db" }
sqlx = { workspace = true }
```

- [ ] **Step 2: Create `attachment.rs` with the shared upload function**

Create `backend/crates/sober-workspace/src/attachment.rs`:

```rust
//! Shared attachment processing and storage pipeline.
//!
//! Used by both `sober-api` (HTTP uploads) and `sober-agent` (gRPC uploads from gateway).

use std::time::Instant;

use metrics::{counter, histogram};
use sober_core::error::AppError;
use sober_core::types::{
    AttachmentKind, ConversationAttachment, ConversationAttachmentRepo, ConversationId,
    CreateConversationAttachment, UserId,
};
use sober_db::PgConversationAttachmentRepo;
use sqlx::PgPool;
use tracing::instrument;

use crate::BlobStore;
use crate::image_processing;
use crate::text_extraction;

/// Validates, processes, and stores a file attachment.
///
/// Pipeline: validate content type via magic bytes → derive kind → process
/// (resize images / extract document text) → store blob → create DB record.
///
/// Does **not** verify conversation membership — callers must do that if needed.
#[instrument(skip(db, blob_store, data), fields(conversation.id = %conversation_id, attachment.filename = %filename))]
pub async fn process_and_store_attachment(
    db: &PgPool,
    blob_store: &BlobStore,
    conversation_id: ConversationId,
    user_id: UserId,
    filename: String,
    data: Vec<u8>,
) -> Result<ConversationAttachment, AppError> {
    let start = Instant::now();

    let content_type = image_processing::validate_content_type(&data)
        .ok_or_else(|| AppError::Validation("unsupported or unrecognised file type".into()))?;

    let kind = image_processing::derive_attachment_kind(content_type);

    let (store_data, final_content_type, metadata) = match kind {
        AttachmentKind::Image => {
            let processed = image_processing::process_image(&data, content_type)
                .map_err(|e| AppError::Internal(e.into()))?;
            let metadata = serde_json::json!({
                "width": processed.width,
                "height": processed.height,
            });
            (processed.data, processed.content_type, metadata)
        }
        AttachmentKind::Document => {
            let extracted = text_extraction::extract_text(&data, content_type)
                .map_err(|e| AppError::Internal(e.into()))?;
            let metadata = match extracted {
                Some(text) => serde_json::json!({ "extracted_text": text }),
                None => serde_json::json!({}),
            };
            (data, content_type.to_string(), metadata)
        }
        _ => (data, content_type.to_string(), serde_json::json!({})),
    };

    let blob_key = blob_store
        .store(&store_data)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let repo = PgConversationAttachmentRepo::new(db.clone());
    let attachment = repo
        .create(CreateConversationAttachment {
            blob_key,
            kind,
            content_type: final_content_type,
            filename,
            size: store_data.len() as i64,
            metadata,
            conversation_id,
            user_id,
        })
        .await?;

    let kind_label = match kind {
        AttachmentKind::Image => "image",
        AttachmentKind::Audio => "audio",
        AttachmentKind::Video => "video",
        AttachmentKind::Document => "document",
    };
    counter!("sober_attachment_uploads_total", "kind" => kind_label, "status" => "success")
        .increment(1);
    histogram!("sober_attachment_upload_bytes").record(store_data.len() as f64);
    histogram!("sober_attachment_upload_duration_seconds")
        .record(start.elapsed().as_secs_f64());

    Ok(attachment)
}
```

- [ ] **Step 3: Export the new module in `lib.rs`**

In `backend/crates/sober-workspace/src/lib.rs`, add:
```rust
pub mod attachment;
```

- [ ] **Step 4: Reduce `sober-api`'s `AttachmentService::upload` to delegate**

Replace `backend/crates/sober-api/src/services/attachment.rs` with:

```rust
use std::sync::Arc;

use sober_core::error::AppError;
use sober_core::types::{ConversationAttachment, ConversationId, UserId};
use sober_workspace::BlobStore;
use sqlx::PgPool;

pub struct AttachmentService {
    db: PgPool,
    blob_store: Arc<BlobStore>,
}

impl AttachmentService {
    pub fn new(db: PgPool, blob_store: Arc<BlobStore>) -> Self {
        Self { db, blob_store }
    }

    /// Process and store an uploaded file attachment.
    pub async fn upload(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        filename: String,
        data: Vec<u8>,
    ) -> Result<ConversationAttachment, AppError> {
        super::verify_membership(&self.db, conversation_id, user_id).await?;

        sober_workspace::attachment::process_and_store_attachment(
            &self.db,
            &self.blob_store,
            conversation_id,
            user_id,
            filename,
            data,
        )
        .await
    }
}
```

- [ ] **Step 5: Build affected crates to verify**

Run: `cd backend && cargo build -q -p sober-workspace -p sober-api`

Expected: compiles with no errors.

- [ ] **Step 6: Run existing tests to verify no regressions**

Run: `cd backend && cargo test -q -p sober-workspace -p sober-api`

Expected: all existing tests pass.

- [ ] **Step 7: Commit**

```bash
git add backend/crates/sober-workspace/src/attachment.rs \
       backend/crates/sober-workspace/src/lib.rs \
       backend/crates/sober-workspace/Cargo.toml \
       backend/crates/sober-api/src/services/attachment.rs
git commit -m "refactor(workspace): extract shared attachment processing pipeline"
```

---

### Task 2: Add attachment RPCs to proto

**Files:**
- Modify: `backend/proto/sober/agent/v1/agent.proto`

- [ ] **Step 1: Add RPC declarations and message types**

In `agent.proto`, add these two RPCs inside the `AgentService` block (after `RevertEvolution`):

```protobuf
  // Upload an attachment from an external source (gateway).
  rpc UploadAttachment(UploadAttachmentRequest) returns (UploadAttachmentResponse);
  // Retrieve attachment content by ID.
  rpc GetAttachmentContent(GetAttachmentContentRequest) returns (GetAttachmentContentResponse);
```

Add these messages at the end of the file:

```protobuf
// --- Attachment RPCs ---

message UploadAttachmentRequest {
  string conversation_id = 1;
  string user_id = 2;
  string filename = 3;
  bytes data = 4;
}

message UploadAttachmentResponse {
  string conversation_attachment_id = 1;
  string kind = 2;
  string content_type = 3;
}

message GetAttachmentContentRequest {
  string conversation_attachment_id = 1;
}

message GetAttachmentContentResponse {
  bytes data = 1;
  string content_type = 2;
  string filename = 3;
  string kind = 4;
}
```

- [ ] **Step 2: Build sober-agent to verify proto codegen**

Run: `cd backend && cargo build -q -p sober-agent 2>&1 | head -20`

Expected: build will fail because the new RPCs are not implemented yet in `AgentGrpcService`. This is expected — we add them in Task 3.

- [ ] **Step 3: Commit**

```bash
git add backend/proto/sober/agent/v1/agent.proto
git commit -m "feat(proto): add UploadAttachment and GetAttachmentContent RPCs"
```

---

### Task 3: Implement attachment RPCs in the agent

**Files:**
- Create: `backend/crates/sober-agent/src/grpc/attachments.rs`
- Modify: `backend/crates/sober-agent/src/grpc/mod.rs`
- Modify: `backend/crates/sober-agent/Cargo.toml` (if `sober-workspace` not already a dep)

- [ ] **Step 1: Verify sober-workspace is a dependency of sober-agent**

Run: `cd backend && grep sober-workspace crates/sober-agent/Cargo.toml`

If not present, add it. (It likely already is, since agent uses `BlobStore` via `ToolBootstrap`.)

- [ ] **Step 2: Create the handler module**

Create `backend/crates/sober-agent/src/grpc/attachments.rs`:

```rust
//! gRPC handlers for attachment upload and retrieval.

use sober_core::types::{AgentRepos, ConversationAttachmentRepo, ConversationAttachmentId, ConversationId, UserId};
use tonic::{Request, Response, Status};

use super::AgentGrpcService;
use super::proto;

/// Handles `UploadAttachment` — stores an attachment from the gateway.
pub(crate) async fn upload_attachment<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    request: Request<proto::UploadAttachmentRequest>,
) -> Result<Response<proto::UploadAttachmentResponse>, Status> {
    let req = request.into_inner();

    let conversation_id = req
        .conversation_id
        .parse::<uuid::Uuid>()
        .map(ConversationId::from_uuid)
        .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;

    let user_id = req
        .user_id
        .parse::<uuid::Uuid>()
        .map(UserId::from_uuid)
        .map_err(|_| Status::invalid_argument("invalid user_id"))?;

    if req.data.is_empty() {
        return Err(Status::invalid_argument("attachment data is empty"));
    }

    let bootstrap = service.agent().tool_bootstrap();
    let db = bootstrap.repos.db_pool();
    let blob_store = &bootstrap.blob_store;

    let attachment = sober_workspace::attachment::process_and_store_attachment(
        db,
        blob_store,
        conversation_id,
        user_id,
        req.filename,
        req.data,
    )
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    let kind = match attachment.kind {
        sober_core::types::AttachmentKind::Image => "image",
        sober_core::types::AttachmentKind::Audio => "audio",
        sober_core::types::AttachmentKind::Video => "video",
        sober_core::types::AttachmentKind::Document => "document",
    };

    Ok(Response::new(proto::UploadAttachmentResponse {
        conversation_attachment_id: attachment.id.to_string(),
        kind: kind.to_owned(),
        content_type: attachment.content_type,
    }))
}

/// Handles `GetAttachmentContent` — retrieves attachment bytes for the gateway.
pub(crate) async fn get_attachment_content<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    request: Request<proto::GetAttachmentContentRequest>,
) -> Result<Response<proto::GetAttachmentContentResponse>, Status> {
    let req = request.into_inner();

    let attachment_id = req
        .conversation_attachment_id
        .parse::<uuid::Uuid>()
        .map(ConversationAttachmentId::from_uuid)
        .map_err(|_| Status::invalid_argument("invalid conversation_attachment_id"))?;

    let attachment = service
        .agent()
        .repos()
        .attachments()
        .get_by_id(attachment_id)
        .await
        .map_err(|e| Status::not_found(e.to_string()))?;

    let bootstrap = service.agent().tool_bootstrap();
    let data = bootstrap
        .blob_store
        .retrieve(&attachment.blob_key)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let kind = match attachment.kind {
        sober_core::types::AttachmentKind::Image => "image",
        sober_core::types::AttachmentKind::Audio => "audio",
        sober_core::types::AttachmentKind::Video => "video",
        sober_core::types::AttachmentKind::Document => "document",
    };

    Ok(Response::new(proto::GetAttachmentContentResponse {
        data,
        content_type: attachment.content_type,
        filename: attachment.filename,
        kind: kind.to_owned(),
    }))
}
```

**Note:** The `db_pool()` method on `AgentRepos` may not exist yet. Check the `AgentRepos` trait and the concrete implementation. If it doesn't have a pool accessor, you'll need to either:
- Add `fn db_pool(&self) -> &PgPool` to `AgentRepos`
- Or access the pool via the `ToolBootstrap` if it's stored there
- Or construct a `PgConversationAttachmentRepo` directly from the pool stored in `ToolBootstrap`

Look at how `ToolBootstrap` stores its repos: `pub repos: Arc<R>`. The concrete type likely wraps a `PgPool`. Check `PgAgentRepos` (or whatever the concrete impl is called) for a `db_pool()` or `pool()` method. If needed, add one.

- [ ] **Step 3: Register the module and wire up the RPC dispatchers**

In `backend/crates/sober-agent/src/grpc/mod.rs`, add the module declaration:
```rust
mod attachments;
```

In the `impl AgentService for AgentGrpcService<R>` block, add:

```rust
    async fn upload_attachment(
        &self,
        request: Request<proto::UploadAttachmentRequest>,
    ) -> Result<Response<proto::UploadAttachmentResponse>, Status> {
        attachments::upload_attachment(self, request).await
    }

    async fn get_attachment_content(
        &self,
        request: Request<proto::GetAttachmentContentRequest>,
    ) -> Result<Response<proto::GetAttachmentContentResponse>, Status> {
        attachments::get_attachment_content(self, request).await
    }
```

- [ ] **Step 4: Increase gRPC message size limits on the agent server**

Find where the agent's tonic server is constructed in `backend/crates/sober-agent/src/main.rs`. It will look like `Server::builder().add_service(...)`. Add message size limits:

```rust
.max_encoding_message_size(50 * 1024 * 1024)
.max_decoding_message_size(50 * 1024 * 1024)
```

This is needed because attachments can be up to 25 MB, exceeding tonic's default 4 MB limit.

- [ ] **Step 5: Build to verify**

Run: `cd backend && cargo build -q -p sober-agent`

Expected: compiles with no errors.

- [ ] **Step 6: Run clippy**

Run: `cd backend && cargo clippy -q -p sober-agent -- -D warnings`

Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add backend/crates/sober-agent/src/grpc/attachments.rs \
       backend/crates/sober-agent/src/grpc/mod.rs \
       backend/crates/sober-agent/src/main.rs
git commit -m "feat(agent): implement UploadAttachment and GetAttachmentContent RPCs"
```

---

### Task 4: Extend gateway types with attachment support

**Files:**
- Modify: `backend/crates/sober-gateway/src/types.rs`
- Modify: `backend/crates/sober-gateway/src/error.rs`

- [ ] **Step 1: Add attachment structs and extend existing types**

In `backend/crates/sober-gateway/src/types.rs`, add the structs and modify `GatewayEvent` and `PlatformMessage`:

```rust
/// An attachment downloaded from an external platform.
#[derive(Debug)]
pub struct InboundAttachment {
    /// Original filename from the platform.
    pub filename: String,
    /// MIME content type reported by the platform.
    pub content_type: Option<String>,
    /// Raw file bytes (already downloaded from platform CDN).
    pub data: Vec<u8>,
}

/// An attachment to send to an external platform.
#[derive(Debug, Clone)]
pub struct OutboundAttachment {
    /// Filename to present on the platform.
    pub filename: String,
    /// MIME content type.
    pub content_type: String,
    /// Raw file bytes.
    pub data: Vec<u8>,
}
```

Add `attachments: Vec<InboundAttachment>` to `GatewayEvent::MessageReceived`.

Add `attachments: Vec<OutboundAttachment>` to `PlatformMessage`.

- [ ] **Step 2: Add attachment error variants**

In `backend/crates/sober-gateway/src/error.rs`, add:

```rust
    #[error("attachment download failed: {0}")]
    AttachmentDownloadFailed(String),

    #[error("attachment upload failed: {0}")]
    AttachmentUploadFailed(String),

    #[error("attachment fetch failed: {0}")]
    AttachmentFetchFailed(String),
```

- [ ] **Step 3: Fix all compilation errors from the new fields**

The new fields on `GatewayEvent::MessageReceived` and `PlatformMessage` will cause compilation errors at every construction site. Fix each one:

- `backend/crates/sober-gateway/src/discord/handler.rs` — add `attachments: vec![]` to the `GatewayEvent::MessageReceived` (we'll populate it in Task 5)
- `backend/crates/sober-gateway/src/helpers.rs` — add `attachments: Vec::new()` to `PlatformMessage` constructions (the outbound `PlatformMessage` in `run_outbound_stream` and the `deliver_outbound` function)
- `backend/crates/sober-gateway/src/outbound.rs` — add `attachments: Vec::new()` to the `PlatformMessage` in `OutboundBuffer::flush()`

Search for all construction sites:

Run: `cd backend && grep -rn 'GatewayEvent::MessageReceived' crates/sober-gateway/src/`
Run: `cd backend && grep -rn 'PlatformMessage {' crates/sober-gateway/src/`

Fix every occurrence.

- [ ] **Step 4: Build to verify**

Run: `cd backend && cargo build -q -p sober-gateway`

Expected: compiles with no errors.

- [ ] **Step 5: Run existing tests**

Run: `cd backend && cargo test -q -p sober-gateway`

Expected: all existing tests pass (outbound buffer tests, split_message tests).

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-gateway/src/types.rs \
       backend/crates/sober-gateway/src/error.rs \
       backend/crates/sober-gateway/src/discord/handler.rs \
       backend/crates/sober-gateway/src/helpers.rs \
       backend/crates/sober-gateway/src/outbound.rs
git commit -m "feat(gateway): add attachment types to GatewayEvent and PlatformMessage"
```

---

### Task 5: Discord inbound — download attachments from CDN

**Files:**
- Modify: `backend/crates/sober-gateway/Cargo.toml`
- Modify: `backend/crates/sober-gateway/src/discord/handler.rs`

- [ ] **Step 1: Add `reqwest` dependency**

In `backend/crates/sober-gateway/Cargo.toml`, add to `[dependencies]`:

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
```

- [ ] **Step 2: Implement attachment downloading in the Discord handler**

Replace the `message` method in `backend/crates/sober-gateway/src/discord/handler.rs`:

```rust
    async fn message(&self, _ctx: Context, msg: Message) {
        // Skip messages from bots (including ourselves).
        if msg.author.bot {
            return;
        }

        // Guard: skip if the bot user ID hasn't been set yet.
        let bot_id = *self.bot_user_id.lock().await;
        if let Some(bot_id) = bot_id
            && msg.author.id == bot_id
        {
            return;
        }

        debug!(
            platform_id = %self.platform_id,
            channel_id = %msg.channel_id,
            author = %msg.author.name,
            attachment_count = msg.attachments.len(),
            "received Discord message"
        );

        // Download attachments from Discord CDN.
        let mut attachments = Vec::with_capacity(msg.attachments.len());
        for attachment in &msg.attachments {
            let start = std::time::Instant::now();
            match download_attachment(&attachment.url, &attachment.filename).await {
                Ok(inbound) => {
                    metrics::counter!(
                        "sober_gateway_attachments_downloaded_total",
                        "platform" => "discord",
                        "status" => "success",
                    )
                    .increment(1);
                    metrics::histogram!(
                        "sober_gateway_attachment_download_duration_seconds",
                        "platform" => "discord",
                    )
                    .record(start.elapsed().as_secs_f64());
                    metrics::histogram!(
                        "sober_gateway_attachment_download_bytes",
                        "platform" => "discord",
                    )
                    .record(inbound.data.len() as f64);
                    attachments.push(inbound);
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        filename = %attachment.filename,
                        url = %attachment.url,
                        "failed to download Discord attachment, skipping"
                    );
                    metrics::counter!(
                        "sober_gateway_attachments_downloaded_total",
                        "platform" => "discord",
                        "status" => "error",
                    )
                    .increment(1);
                }
            }
        }

        let event = GatewayEvent::MessageReceived {
            platform_id: self.platform_id,
            channel_id: msg.channel_id.to_string(),
            user_id: msg.author.id.to_string(),
            username: msg.author.name.clone(),
            content: msg.content.clone(),
            attachments,
        };

        if let Err(e) = self.event_tx.send(event).await {
            tracing::error!(error = %e, "failed to forward Discord message to event loop");
        }
    }
```

Add the download helper function in the same file:

```rust
use crate::types::InboundAttachment;

/// Downloads an attachment from a platform CDN URL.
async fn download_attachment(url: &str, filename: &str) -> Result<InboundAttachment, String> {
    let response = reqwest::Client::new()
        .get(url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());

    let data = response
        .bytes()
        .await
        .map_err(|e| format!("failed to read body: {e}"))?
        .to_vec();

    Ok(InboundAttachment {
        filename: filename.to_owned(),
        content_type,
        data,
    })
}
```

- [ ] **Step 3: Build to verify**

Run: `cd backend && cargo build -q -p sober-gateway`

Expected: compiles with no errors.

- [ ] **Step 4: Run clippy**

Run: `cd backend && cargo clippy -q -p sober-gateway -- -D warnings`

Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-gateway/Cargo.toml \
       backend/crates/sober-gateway/src/discord/handler.rs
git commit -m "feat(gateway): download Discord attachments from CDN on inbound messages"
```

---

### Task 6: Gateway service — upload attachments and build content blocks

**Files:**
- Modify: `backend/crates/sober-gateway/src/service.rs`

- [ ] **Step 1: Update `handle_event` to pass attachments through**

In the `GatewayEvent::MessageReceived` match arm in `handle_event`, destructure the new `attachments` field and pass it to `handle_message`:

```rust
GatewayEvent::MessageReceived {
    platform_id,
    channel_id,
    user_id,
    username,
    content,
    attachments,
} => {
    if let Err(e) = self
        .handle_message(platform_id, channel_id, user_id, username, content, attachments)
        .await
    {
        // ... existing error handling
    }
}
```

- [ ] **Step 2: Update `handle_message` to upload attachments and build content blocks**

Add `attachments: Vec<InboundAttachment>` to the `handle_message` parameter list. Import `InboundAttachment` from `crate::types`.

After the user resolution and before building the `HandleMessageRequest`, add attachment upload logic:

```rust
use crate::agent_proto::{ImageBlock, FileBlock, AudioBlock, VideoBlock};
use crate::types::InboundAttachment;

// Upload attachments to the agent and build content blocks.
let mut content_blocks = Vec::new();

if !content.is_empty() {
    content_blocks.push(ContentBlock {
        block: Some(Block::Text(TextBlock { text: content })),
    });
}

for attachment in attachments {
    let upload_req = crate::agent_proto::UploadAttachmentRequest {
        conversation_id: mapping.conversation_id.to_string(),
        user_id: user_id.to_string(),
        filename: attachment.filename.clone(),
        data: attachment.data,
    };

    let mut client = self.agent_client.clone();
    match client.upload_attachment(upload_req).await {
        Ok(resp) => {
            let resp = resp.into_inner();
            let block = match resp.kind.as_str() {
                "image" => Some(Block::Image(ImageBlock {
                    conversation_attachment_id: resp.conversation_attachment_id.clone(),
                    alt: Some(attachment.filename),
                })),
                "audio" => Some(Block::Audio(AudioBlock {
                    conversation_attachment_id: resp.conversation_attachment_id.clone(),
                })),
                "video" => Some(Block::Video(VideoBlock {
                    conversation_attachment_id: resp.conversation_attachment_id.clone(),
                })),
                _ => Some(Block::File(FileBlock {
                    conversation_attachment_id: resp.conversation_attachment_id.clone(),
                })),
            };
            if let Some(block) = block {
                content_blocks.push(ContentBlock { block: Some(block) });
            }
            let platform_label = self
                .bridge_registry
                .get(&platform_id)
                .map(|b| b.platform_type().to_string())
                .unwrap_or_else(|| "unknown".to_owned());
            metrics::counter!(
                "sober_gateway_attachments_uploaded_total",
                "platform" => platform_label,
                "kind" => resp.kind,
                "status" => "success",
            )
            .increment(1);
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                filename = %attachment.filename,
                "failed to upload attachment to agent, skipping"
            );
        }
    }
}

// Skip if there's nothing to send (no text and all attachments failed).
if content_blocks.is_empty() {
    return Ok(());
}
```

Then update the `HandleMessageRequest` to use `content_blocks` instead of the hardcoded single text block:

```rust
let request = HandleMessageRequest {
    user_id: user_id.to_string(),
    conversation_id: mapping.conversation_id.to_string(),
    content: content_blocks,
    source: "gateway".to_owned(),
};
```

- [ ] **Step 3: Increase gRPC message size on the gateway's agent client**

Find where the `AgentServiceClient` is constructed (in `main.rs` or `service.rs`). Add message size limits to the channel:

```rust
.max_encoding_message_size(50 * 1024 * 1024)
.max_decoding_message_size(50 * 1024 * 1024)
```

Search for the construction site:

Run: `cd backend && grep -rn 'AgentServiceClient' crates/sober-gateway/src/`

- [ ] **Step 4: Build to verify**

Run: `cd backend && cargo build -q -p sober-gateway`

Expected: compiles with no errors.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-gateway/src/service.rs \
       backend/crates/sober-gateway/src/main.rs
git commit -m "feat(gateway): upload inbound attachments to agent and build content blocks"
```

---

### Task 7: Discord outbound — send files via Serenity

**Files:**
- Modify: `backend/crates/sober-gateway/src/discord/client.rs`

- [ ] **Step 1: Update `send_message` to handle attachments**

Replace the `send_message` implementation in the `PlatformBridgeHandle` impl:

```rust
    async fn send_message(
        &self,
        channel_id: &str,
        content: PlatformMessage,
    ) -> Result<(), GatewayError> {
        let channel_id: u64 = channel_id.parse().map_err(|_| {
            GatewayError::ChannelNotFound(format!("invalid Discord channel ID: {channel_id}"))
        })?;

        let channel = ChannelId::new(channel_id);

        if content.attachments.is_empty() {
            // Text-only path — existing behaviour.
            for chunk in split_message(&content.text, DISCORD_MAX_LEN) {
                channel
                    .say(&self.http, chunk)
                    .await
                    .map_err(|e| GatewayError::SendFailed(e.to_string()))?;
            }
        } else {
            // Send with file attachments. Discord allows max 10 files per message.
            use serenity::builder::{CreateAttachment, CreateMessage};

            for file_chunk in content.attachments.chunks(10) {
                let mut msg = CreateMessage::new();

                // Include text only in the first chunk.
                if !content.text.is_empty() {
                    // Truncate text to Discord's limit — attachments take priority.
                    let text = if content.text.len() > DISCORD_MAX_LEN {
                        &content.text[..DISCORD_MAX_LEN]
                    } else {
                        &content.text
                    };
                    msg = msg.content(text);
                }

                for attachment in file_chunk {
                    msg = msg.add_file(CreateAttachment::bytes(
                        attachment.data.clone(),
                        &attachment.filename,
                    ));
                }

                channel
                    .send_message(&self.http, msg)
                    .await
                    .map_err(|e| GatewayError::SendFailed(e.to_string()))?;
            }
        }

        Ok(())
    }
```

Add the necessary import at the top of the file if not present:
```rust
use serenity::builder::{CreateAttachment, CreateMessage};
```

- [ ] **Step 2: Build to verify**

Run: `cd backend && cargo build -q -p sober-gateway`

Expected: compiles with no errors.

- [ ] **Step 3: Run existing tests**

Run: `cd backend && cargo test -q -p sober-gateway`

Expected: all existing tests pass.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-gateway/src/discord/client.rs
git commit -m "feat(gateway): send file attachments via Discord CreateAttachment"
```

---

### Task 8: Outbound stream — detect and deliver media from agent responses

**Files:**
- Modify: `backend/crates/sober-gateway/src/helpers.rs`

- [ ] **Step 1: Thread the agent channel through to enable attachment fetching**

The `run_outbound_stream` function already receives `agent_channel: tonic::transport::Channel`. It creates an `AgentServiceClient` from it. Store the client so it can be reused for `GetAttachmentContent` calls.

Change the local `client` variable to remain available throughout the function (it's currently scoped to the subscription call). Move it to the outer scope:

```rust
let mut client = AgentServiceClient::new(agent_channel)
    .max_decoding_message_size(50 * 1024 * 1024)
    .max_encoding_message_size(50 * 1024 * 1024);
```

- [ ] **Step 2: Add media detection in the NewMessage handler for assistant messages**

In `run_outbound_stream`, add a new match arm for assistant `NewMessage` events. Currently only `role == "user"` is handled. Add after the user message handler:

```rust
Some(Event::NewMessage(ref nm)) if nm.role.to_lowercase() == "assistant" => {
    // Check for non-text content blocks (images, files, etc.)
    let non_text_blocks: Vec<_> = nm
        .content
        .iter()
        .filter_map(|b| match b.block.as_ref()? {
            Block::Text(_) => None,
            other => Some(other.clone()),
        })
        .collect();

    if non_text_blocks.is_empty() {
        continue;
    }

    // Fetch attachment data for each non-text block.
    let mut outbound_attachments = Vec::new();
    for block in &non_text_blocks {
        let attachment_id = match block {
            Block::Image(img) => &img.conversation_attachment_id,
            Block::File(f) => &f.conversation_attachment_id,
            Block::Audio(a) => &a.conversation_attachment_id,
            Block::Video(v) => &v.conversation_attachment_id,
            Block::Text(_) => continue,
        };

        let fetch_req = sober_gateway::agent_proto::GetAttachmentContentRequest {
            conversation_attachment_id: attachment_id.clone(),
        };

        match client.get_attachment_content(fetch_req).await {
            Ok(resp) => {
                let resp = resp.into_inner();
                outbound_attachments.push(sober_gateway::types::OutboundAttachment {
                    filename: resp.filename,
                    content_type: resp.content_type,
                    data: resp.data,
                });
                metrics::counter!(
                    "sober_gateway_attachments_fetched_total",
                    "kind" => resp.kind,
                    "status" => "success",
                )
                .increment(1);
            }
            Err(e) => {
                error!(
                    error = %e,
                    attachment_id = %attachment_id,
                    "failed to fetch attachment content, skipping"
                );
                metrics::counter!(
                    "sober_gateway_attachments_fetched_total",
                    "kind" => "unknown",
                    "status" => "error",
                )
                .increment(1);
            }
        }
    }

    if !outbound_attachments.is_empty() {
        let msg = sober_gateway::types::PlatformMessage {
            text: String::new(),
            format: sober_gateway::types::MessageFormat::Markdown,
            reply_to: None,
            attachments: outbound_attachments,
        };
        deliver_outbound(service.as_ref(), conversation_id, msg).await;
    }
}
```

Make sure to import the `Block` type at the top of the function or file:
```rust
use sober_gateway::agent_proto::content_block::Block;
```

- [ ] **Step 3: Build to verify**

Run: `cd backend && cargo build -q -p sober-gateway`

Expected: compiles with no errors. (The gateway binary crate links the lib — helpers.rs is in the binary crate.)

- [ ] **Step 4: Run clippy on all modified crates**

Run: `cd backend && cargo clippy -q -p sober-gateway -- -D warnings`

Expected: no warnings.

- [ ] **Step 5: Run all tests**

Run: `cd backend && cargo test -q -p sober-gateway`

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-gateway/src/helpers.rs
git commit -m "feat(gateway): detect and deliver media attachments in outbound agent responses"
```

---

### Task 9: Metrics declarations

**Files:**
- Modify: `backend/crates/sober-gateway/metrics.toml` (if it exists)

- [ ] **Step 1: Check if metrics.toml exists**

Run: `ls backend/crates/sober-gateway/metrics.toml 2>/dev/null || echo "not found"`

If it exists, add the new metrics. If not, check the project-level metrics file:

Run: `find backend -name 'metrics.toml' -type f`

- [ ] **Step 2: Declare the 5 new metrics**

Add to the appropriate metrics declaration file:

```toml
[[metrics]]
name = "sober_gateway_attachments_downloaded_total"
type = "counter"
labels = ["platform", "status"]
description = "Number of attachments downloaded from platform CDNs"

[[metrics]]
name = "sober_gateway_attachments_uploaded_total"
type = "counter"
labels = ["platform", "kind", "status"]
description = "Number of attachments uploaded to the agent via gRPC"

[[metrics]]
name = "sober_gateway_attachments_fetched_total"
type = "counter"
labels = ["kind", "status"]
description = "Number of attachments fetched from the agent for outbound delivery"

[[metrics]]
name = "sober_gateway_attachment_download_duration_seconds"
type = "histogram"
labels = ["platform"]
description = "Time to download attachments from platform CDNs"

[[metrics]]
name = "sober_gateway_attachment_download_bytes"
type = "histogram"
labels = ["platform"]
description = "Size of downloaded attachments in bytes"
```

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-gateway/metrics.toml  # or wherever the metrics file is
git commit -m "docs(gateway): declare attachment metrics"
```

---

### Task 10: Final verification and workspace-wide build

- [ ] **Step 1: Run full workspace build**

Run: `cd backend && cargo build -q --workspace`

Expected: all crates compile.

- [ ] **Step 2: Run full workspace clippy**

Run: `cd backend && cargo clippy -q --workspace -- -D warnings`

Expected: no warnings.

- [ ] **Step 3: Run full workspace tests**

Run: `cd backend && cargo test -q --workspace`

Expected: all tests pass.

- [ ] **Step 4: Run frontend checks (no changes expected, sanity check)**

Run: `cd frontend && pnpm check && pnpm test --silent`

Expected: passes (no frontend changes in this plan).
