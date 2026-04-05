# #053: Gateway Attachment Support — Bidirectional Media

## Overview

Enable bidirectional image/file/audio/video support in the gateway bridge.
Currently the gateway is text-only in both directions: inbound (Discord handler
ignores `msg.attachments`) and outbound (`PlatformMessage` only carries text).
The proto contract (`ContentBlock` with `ImageBlock`, `FileBlock`, etc.) and
the agent's multimodal pipeline (blob storage, image processing, vision) already
support attachments — only the gateway layer needs wiring.

```
Inbound:  Discord attachment → download CDN → UploadAttachment RPC → blob store → ContentBlock → agent
Outbound: agent NewMessage w/ ImageBlock → GetAttachmentContent RPC → fetch blob → platform send_files
```

## Architecture

### Communication — Two New Agent RPCs

The gateway communicates with the agent exclusively via gRPC/UDS. Rather than
introducing a new cross-service HTTP dependency (gateway → sober-api), add two
RPCs to the existing `AgentService`:

| RPC | Direction | Purpose |
|-----|-----------|---------|
| `UploadAttachment` | gateway → agent | Send raw bytes + metadata, receive `conversation_attachment_id` |
| `GetAttachmentContent` | gateway → agent | Send attachment ID, receive blob bytes + metadata |

This keeps the gateway thin (no blob store, no DB writes) and follows the
existing pattern where `HandleMessage` and `SubscribeConversationUpdates` are
the gateway's only agent touchpoints.

**Why not reuse sober-api's HTTP endpoints?**
- Adds auth token management and a new service dependency
- Co-located UDS is faster and simpler than HTTP
- The agent already has `BlobStore` and `PgConversationAttachmentRepo`

### Shared Attachment Logic

The core upload pipeline (validate content type → derive kind → process image →
store blob → create DB record) currently lives in `sober-api/src/services/attachment.rs`.
Extract it into `sober-workspace::attachment` so both sober-api and sober-agent
can reuse it without duplication.

```rust
// sober-workspace/src/attachment.rs
pub async fn process_and_store_attachment(
    db: &PgPool,
    blob_store: &BlobStore,
    conversation_id: ConversationId,
    user_id: UserId,
    filename: String,
    data: Vec<u8>,
) -> Result<ConversationAttachment, AppError>
```

sober-api's `AttachmentService::upload` becomes: verify membership → call this function.

### gRPC Message Size

Default gRPC limit is 4 MB. Discord allows 25 MB attachments (100 MB with Nitro).
Configure `max_encoding_message_size(50 MB)` and `max_decoding_message_size(50 MB)`
on both the agent server and gateway client. Safe since they communicate over a
co-located Unix domain socket.

### Gateway Types

```rust
// Inbound: platform → agent
pub struct InboundAttachment {
    pub filename: String,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

// Outbound: agent → platform
pub struct OutboundAttachment {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}
```

`GatewayEvent::MessageReceived` gains `attachments: Vec<InboundAttachment>`.
`PlatformMessage` gains `attachments: Vec<OutboundAttachment>`.

### Inbound Flow

```
Discord msg.attachments
  → reqwest GET from CDN (30s timeout, parallel)
  → InboundAttachment { filename, content_type, data }
  → GatewayEvent::MessageReceived { ..., attachments }
  → GatewayService::handle_event()
      → UploadAttachment RPC (parallel per attachment)
      → Build ContentBlock per returned kind
      → HandleMessage RPC with text + attachment blocks
```

If an attachment download fails, log a warning and skip it — text content is
still forwarded. This matches web frontend behaviour where partial uploads
don't block the message.

### Outbound Flow

```
SubscribeConversationUpdates stream
  → Event::NewMessage (role: "assistant")
      → Check content for non-text blocks (ImageBlock, FileBlock, etc.)
      → GetAttachmentContent RPC per attachment (parallel)
      → Build OutboundAttachment { filename, content_type, data }
      → PlatformMessage { text: "", attachments }
      → deliver_outbound()
```

Text is already delivered via the `TextDelta`/`Done` streaming path. Media
blocks in the `NewMessage` event are delivered as a follow-up message with
attachments. This avoids changing the streaming text flow.

### Platform-Specific Limits

| Platform | Max file size | Max files/msg | Send API |
|----------|--------------|---------------|----------|
| Discord | 25 MB (100 MB Nitro) | 10 | `CreateMessage` + `CreateAttachment::bytes` |
| Telegram | 50 MB (photo: 10 MB) | 10 (media group) | `sendPhoto` / `sendDocument` |
| Matrix | 50 MB (homeserver-dependent) | 1 per event | `upload` → `m.image` event |
| WhatsApp | 16 MB (images: 5 MB) | 1 | Media upload → send media message |

The `PlatformBridgeHandle` trait doesn't change — `send_message` already takes
`PlatformMessage`, and each implementation handles attachments per its platform's
API. If attachments exceed the per-message limit, split across multiple sends.

### Proto Changes

```protobuf
// Added to AgentService
rpc UploadAttachment(UploadAttachmentRequest) returns (UploadAttachmentResponse);
rpc GetAttachmentContent(GetAttachmentContentRequest) returns (GetAttachmentContentResponse);

message UploadAttachmentRequest {
  string conversation_id = 1;
  string user_id = 2;
  string filename = 3;
  bytes data = 4;
}

message UploadAttachmentResponse {
  string conversation_attachment_id = 1;
  string kind = 2;        // "image", "document", "audio", "video"
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

### Metrics

| Metric | Type | Labels |
|--------|------|--------|
| `sober_gateway_attachments_downloaded_total` | counter | platform, status |
| `sober_gateway_attachments_uploaded_total` | counter | platform, kind, status |
| `sober_gateway_attachments_fetched_total` | counter | kind, status |
| `sober_gateway_attachment_download_duration_seconds` | histogram | platform |
| `sober_gateway_attachment_download_bytes` | histogram | platform |

### Error Handling

New `GatewayError` variants:

- `AttachmentDownloadFailed(String)` — CDN download failure (timeout, 404, etc.)
- `AttachmentUploadFailed(String)` — UploadAttachment RPC failure
- `AttachmentFetchFailed(String)` — GetAttachmentContent RPC failure

All are non-fatal: text content is still processed/delivered. Failed attachments
are logged and metered but do not block the message pipeline.

## Affected Crates

| Crate | Changes |
|-------|---------|
| `sober-workspace` | New `attachment` module with shared upload logic |
| `sober-api` | Thin wrapper around shared attachment function |
| `sober-agent` | Implement `UploadAttachment` + `GetAttachmentContent` RPCs |
| `sober-gateway` | Types, Discord handler, service, outbound loop, error variants |

## Not in Scope

- **Inline image generation** — agent doesn't generate images today; this plan
  handles existing attachment references in responses.
- **Sticker/emoji/reaction** support — platform-specific UX, separate plan.
- **Video/audio transcription** — agent treats these as placeholders today.
- **New platform implementations** — Telegram/Matrix/WhatsApp bridge handlers
  are separate work; this plan makes the attachment infrastructure generic so
  they can plug in.
