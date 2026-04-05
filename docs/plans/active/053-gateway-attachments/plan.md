# Gateway Attachment Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable bidirectional image/file/audio/video support between external messaging platforms (Discord) and the agent via the gateway bridge.

**Architecture:** Extract the shared attachment processing pipeline into `sober-workspace`. The gateway calls it directly (it already has `PgPool`, just add `BlobStore`). No new agent RPCs — the agent stays focused on LLM orchestration. Inbound: gateway downloads from CDN, stores blob, builds content blocks for `HandleMessage`. Outbound: gateway detects media in `NewMessage`, fetches blob directly, sends to platform.

**Tech Stack:** Rust, tonic/prost (gRPC), serenity (Discord), reqwest (HTTP downloads), sober-workspace (blob storage + image processing)

---

## File Structure

| File | Responsibility |
|------|---------------|
| `backend/crates/sober-workspace/src/attachment.rs` | **New.** Shared `process_and_store_attachment()` function — validate, process, blob-store, create DB record. |
| `backend/crates/sober-gateway/src/types.rs` | Add `InboundAttachment`, `OutboundAttachment` structs; extend `GatewayEvent` + `PlatformMessage`. |
| `backend/crates/sober-gateway/src/error.rs` | Add attachment error variants. |
| `backend/crates/sober-gateway/src/service.rs` | Add `BlobStore` field; store inbound attachments directly, build content blocks. |
| `backend/crates/sober-gateway/src/discord/handler.rs` | Download `msg.attachments` from Discord CDN. |
| `backend/crates/sober-gateway/src/discord/client.rs` | Send files via Serenity `CreateAttachment`. |
| `backend/crates/sober-gateway/src/helpers.rs` | Detect media in outbound `NewMessage`, fetch blob directly, deliver. |

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
//! Used by both `sober-api` (HTTP uploads) and `sober-gateway` (platform uploads).

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

### Task 2: Extend gateway types with attachment support

**Files:**
- Modify: `backend/crates/sober-gateway/src/types.rs`
- Modify: `backend/crates/sober-gateway/src/error.rs`

- [ ] **Step 1: Add attachment structs and extend existing types**

In `backend/crates/sober-gateway/src/types.rs`, add the structs:

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

    #[error("attachment store failed: {0}")]
    AttachmentStoreFailed(String),

    #[error("attachment fetch failed: {0}")]
    AttachmentFetchFailed(String),
```

- [ ] **Step 3: Fix all compilation errors from the new fields**

The new fields on `GatewayEvent::MessageReceived` and `PlatformMessage` will cause compilation errors at every construction site. Fix each one by adding `attachments: vec![]` / `attachments: Vec::new()`.

Search for all construction sites:

Run: `cd backend && grep -rn 'GatewayEvent::MessageReceived' crates/sober-gateway/src/`
Run: `cd backend && grep -rn 'PlatformMessage {' crates/sober-gateway/src/`

Fix every occurrence:
- `discord/handler.rs` — add `attachments: vec![]` (populated in Task 4)
- `helpers.rs` — add `attachments: Vec::new()` to `PlatformMessage` constructions
- `outbound.rs` — add `attachments: Vec::new()` in `OutboundBuffer::flush()`

- [ ] **Step 4: Build to verify**

Run: `cd backend && cargo build -q -p sober-gateway`

Expected: compiles with no errors.

- [ ] **Step 5: Run existing tests**

Run: `cd backend && cargo test -q -p sober-gateway`

Expected: all existing tests pass.

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

### Task 3: Add `BlobStore` to `GatewayService`

**Files:**
- Modify: `backend/crates/sober-gateway/Cargo.toml`
- Modify: `backend/crates/sober-gateway/src/service.rs`
- Modify: `backend/crates/sober-gateway/src/main.rs`

- [ ] **Step 1: Add `sober-workspace` dependency to gateway**

In `backend/crates/sober-gateway/Cargo.toml`, add:
```toml
sober-workspace = { path = "../sober-workspace" }
```

- [ ] **Step 2: Add `blob_store` field to `GatewayService`**

In `backend/crates/sober-gateway/src/service.rs`, add the field to the struct:

```rust
use std::sync::Arc;
use sober_workspace::BlobStore;

pub struct GatewayService {
    db: PgPool,
    agent_client: AgentServiceClient<tonic::transport::Channel>,
    bridge_registry: Arc<PlatformBridgeRegistry>,
    event_tx: mpsc::Sender<GatewayEvent>,
    blob_store: Arc<BlobStore>,
    // ... existing caches
}
```

Update the `new()` constructor to accept and store `blob_store: Arc<BlobStore>`.

Add a getter:
```rust
    /// Returns the blob store for attachment retrieval.
    pub fn blob_store(&self) -> &Arc<BlobStore> {
        &self.blob_store
    }
```

- [ ] **Step 3: Construct `BlobStore` in `main.rs` and pass to `GatewayService`**

In `backend/crates/sober-gateway/src/main.rs`, construct the blob store from the workspace root (same path the agent uses):

```rust
use sober_workspace::BlobStore;

let blob_store = Arc::new(BlobStore::new(
    workspace_root.join(sober_workspace::SOBER_DIR).join("blobs"),
));
```

Find where `workspace_root` is resolved in the gateway's main — it likely reads from config or env. If not, check how the agent resolves it and use the same source. Pass `blob_store` to `GatewayService::new()`.

- [ ] **Step 4: Build to verify**

Run: `cd backend && cargo build -q -p sober-gateway`

Expected: compiles with no errors.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-gateway/Cargo.toml \
       backend/crates/sober-gateway/src/service.rs \
       backend/crates/sober-gateway/src/main.rs
git commit -m "feat(gateway): add BlobStore to GatewayService for direct attachment access"
```

---

### Task 4: Discord inbound — download attachments from CDN

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

### Task 5: Gateway service — store attachments and build content blocks

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

- [ ] **Step 2: Update `handle_message` to store attachments and build content blocks**

Add `attachments: Vec<InboundAttachment>` to the `handle_message` parameter list. Import `InboundAttachment` from `crate::types`.

After the user resolution and before building the `HandleMessageRequest`, add attachment storage logic:

```rust
use crate::agent_proto::{ImageBlock, FileBlock, AudioBlock, VideoBlock};
use crate::types::InboundAttachment;
use sober_core::types::AttachmentKind;

// Store attachments and build content blocks.
let mut content_blocks = Vec::new();

if !content.is_empty() {
    content_blocks.push(ContentBlock {
        block: Some(Block::Text(TextBlock { text: content })),
    });
}

for attachment in attachments {
    match sober_workspace::attachment::process_and_store_attachment(
        &self.db,
        &self.blob_store,
        mapping.conversation_id,
        user_id,
        attachment.filename.clone(),
        attachment.data,
    )
    .await
    {
        Ok(stored) => {
            let block = match stored.kind {
                AttachmentKind::Image => Block::Image(ImageBlock {
                    conversation_attachment_id: stored.id.to_string(),
                    alt: Some(attachment.filename),
                }),
                AttachmentKind::Audio => Block::Audio(AudioBlock {
                    conversation_attachment_id: stored.id.to_string(),
                }),
                AttachmentKind::Video => Block::Video(VideoBlock {
                    conversation_attachment_id: stored.id.to_string(),
                }),
                AttachmentKind::Document => Block::File(FileBlock {
                    conversation_attachment_id: stored.id.to_string(),
                }),
            };
            content_blocks.push(ContentBlock { block: Some(block) });

            let platform_label = self
                .bridge_registry
                .get(&platform_id)
                .map(|b| b.platform_type().to_string())
                .unwrap_or_else(|| "unknown".to_owned());
            let kind_label = match stored.kind {
                AttachmentKind::Image => "image",
                AttachmentKind::Audio => "audio",
                AttachmentKind::Video => "video",
                AttachmentKind::Document => "document",
            };
            metrics::counter!(
                "sober_gateway_attachments_stored_total",
                "platform" => platform_label,
                "kind" => kind_label,
                "status" => "success",
            )
            .increment(1);
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                filename = %attachment.filename,
                "failed to store attachment, skipping"
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

- [ ] **Step 3: Build to verify**

Run: `cd backend && cargo build -q -p sober-gateway`

Expected: compiles with no errors.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-gateway/src/service.rs
git commit -m "feat(gateway): store inbound attachments and build content blocks directly"
```

---

### Task 6: Discord outbound — send files via Serenity

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

                if !content.text.is_empty() {
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

### Task 7: Outbound stream — detect and deliver media from agent responses

**Files:**
- Modify: `backend/crates/sober-gateway/src/helpers.rs`

- [ ] **Step 1: Add media detection in the NewMessage handler for assistant messages**

In `run_outbound_stream`, add a new match arm for assistant `NewMessage` events. Currently only `role == "user"` is handled. Add after the user message handler:

```rust
Some(Event::NewMessage(ref nm)) if nm.role.to_lowercase() == "assistant" => {
    use sober_gateway::agent_proto::content_block::Block;

    // Check for non-text content blocks (images, files, etc.)
    let attachment_ids: Vec<(String, &str)> = nm
        .content
        .iter()
        .filter_map(|b| match b.block.as_ref()? {
            Block::Image(img) => Some((img.conversation_attachment_id.clone(), "image")),
            Block::File(f) => Some((f.conversation_attachment_id.clone(), "document")),
            Block::Audio(a) => Some((a.conversation_attachment_id.clone(), "audio")),
            Block::Video(v) => Some((v.conversation_attachment_id.clone(), "video")),
            Block::Text(_) => None,
        })
        .collect();

    if attachment_ids.is_empty() {
        continue;
    }

    // Fetch attachment data directly from DB + blob store.
    let mut outbound_attachments = Vec::new();
    for (attachment_id_str, kind_label) in &attachment_ids {
        let Ok(uuid) = attachment_id_str.parse::<uuid::Uuid>() else {
            warn!(attachment_id = %attachment_id_str, "invalid attachment UUID in outbound message");
            continue;
        };
        let attachment_id = sober_core::types::ConversationAttachmentId::from_uuid(uuid);

        let repo = sober_db::PgConversationAttachmentRepo::new(service.db().clone());
        match repo.get_by_id(attachment_id).await {
            Ok(attachment) => {
                match service.blob_store().retrieve(&attachment.blob_key).await {
                    Ok(data) => {
                        outbound_attachments.push(sober_gateway::types::OutboundAttachment {
                            filename: attachment.filename,
                            content_type: attachment.content_type,
                            data,
                        });
                        metrics::counter!(
                            "sober_gateway_attachments_fetched_total",
                            "kind" => *kind_label,
                            "status" => "success",
                        )
                        .increment(1);
                    }
                    Err(e) => {
                        error!(error = %e, attachment_id = %attachment_id_str, "failed to retrieve blob");
                        metrics::counter!(
                            "sober_gateway_attachments_fetched_total",
                            "kind" => *kind_label,
                            "status" => "error",
                        )
                        .increment(1);
                    }
                }
            }
            Err(e) => {
                error!(error = %e, attachment_id = %attachment_id_str, "failed to fetch attachment metadata");
                metrics::counter!(
                    "sober_gateway_attachments_fetched_total",
                    "kind" => *kind_label,
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

**Note:** This requires `GatewayService` to expose a `db()` getter returning `&PgPool`. Add it if not present:

```rust
pub fn db(&self) -> &PgPool {
    &self.db
}
```

- [ ] **Step 2: Build to verify**

Run: `cd backend && cargo build -q -p sober-gateway`

Expected: compiles with no errors.

- [ ] **Step 3: Run clippy on all modified crates**

Run: `cd backend && cargo clippy -q -p sober-gateway -- -D warnings`

Expected: no warnings.

- [ ] **Step 4: Run all tests**

Run: `cd backend && cargo test -q -p sober-gateway`

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-gateway/src/helpers.rs \
       backend/crates/sober-gateway/src/service.rs
git commit -m "feat(gateway): detect and deliver media attachments in outbound agent responses"
```

---

### Task 8: Metrics declarations

**Files:**
- Modify: `backend/crates/sober-gateway/metrics.toml` (if it exists)

- [ ] **Step 1: Check if metrics.toml exists**

Run: `ls backend/crates/sober-gateway/metrics.toml 2>/dev/null || echo "not found"`
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
name = "sober_gateway_attachments_stored_total"
type = "counter"
labels = ["platform", "kind", "status"]
description = "Number of attachments stored via the shared pipeline"

[[metrics]]
name = "sober_gateway_attachments_fetched_total"
type = "counter"
labels = ["kind", "status"]
description = "Number of attachments fetched from blob store for outbound delivery"

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
git add backend/crates/sober-gateway/metrics.toml
git commit -m "docs(gateway): declare attachment metrics"
```

---

### Task 9: Final verification and workspace-wide build

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
