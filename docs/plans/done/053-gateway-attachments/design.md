# #053: Gateway Attachment Support — Bidirectional Media

## Overview

Enable bidirectional image/file/audio/video support in the gateway bridge.
Currently the gateway is text-only in both directions: inbound (Discord handler
ignores `msg.attachments`) and outbound (`PlatformMessage` only carries text).
The proto contract (`ContentBlock` with `ImageBlock`, `FileBlock`, etc.) and
the agent's multimodal pipeline (blob storage, image processing, vision) already
support attachments — only the gateway layer needs wiring.

```
Inbound:  Discord attachment → download CDN → process_and_store_attachment() → ContentBlock → HandleMessage
Outbound: agent NewMessage w/ ImageBlock → gateway fetches blob directly → platform send_files
```

## Architecture

### Direct Blob Access — No Agent RPCs

The gateway already has a `PgPool` (for mappings, user lookups, config). Adding
`Arc<BlobStore>` is one field. With these two, the gateway can call the shared
`process_and_store_attachment()` function directly — no need to proxy through
the agent.

**Why not route through the agent?**
- The agent's job is LLM orchestration, not file storage. Attachment processing
  (validate → resize → store blob → create DB record) is purely mechanical.
- Adding `UploadAttachment` / `GetAttachmentContent` RPCs would make the agent
  a blob proxy, streaming up to 25 MB files through gRPC for no reason.
- The gateway already has DB access. Adding blob access is trivial.

**Why not a separate attachment service?**
- The entire attachment pipeline is one function + one DB table + a
  content-addressed directory. No independent scaling need, no separate
  lifecycle. A whole binary process for that is more infrastructure than logic.

### Shared Attachment Logic

The core upload pipeline (validate content type → derive kind → process image →
store blob → create DB record) currently lives in `sober-api/src/services/attachment.rs`.
Extract it into `sober-workspace::attachment` so both sober-api and sober-gateway
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

### Gateway Types

```rust
// Inbound: platform → gateway → blob store
pub struct InboundAttachment {
    pub filename: String,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

// Outbound: blob store → gateway → platform
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
      → process_and_store_attachment() directly (parallel per attachment)
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
      → attachments().get_by_id() + blob_store.retrieve() directly
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

### Metrics

| Metric | Type | Labels |
|--------|------|--------|
| `sober_gateway_attachments_downloaded_total` | counter | platform, status |
| `sober_gateway_attachments_stored_total` | counter | platform, kind, status |
| `sober_gateway_attachments_fetched_total` | counter | kind, status |
| `sober_gateway_attachment_download_duration_seconds` | histogram | platform |
| `sober_gateway_attachment_download_bytes` | histogram | platform |

### Error Handling

New `GatewayError` variants:

- `AttachmentDownloadFailed(String)` — CDN download failure (timeout, 404, etc.)
- `AttachmentStoreFailed(String)` — process_and_store_attachment failure
- `AttachmentFetchFailed(String)` — blob retrieval failure

All are non-fatal: text content is still processed/delivered. Failed attachments
are logged and metered but do not block the message pipeline.

## Affected Crates

| Crate | Changes |
|-------|---------|
| `sober-workspace` | New `attachment` module with shared upload logic |
| `sober-api` | Thin wrapper around shared attachment function |
| `sober-gateway` | Add `BlobStore`, types, Discord handler, service, outbound loop, error variants |

## Not in Scope

- **Inline image generation** — agent doesn't generate images today; this plan
  handles existing attachment references in responses.
- **Sticker/emoji/reaction** support — platform-specific UX, separate plan.
- **Video/audio transcription** — agent treats these as placeholders today.
- **New platform implementations** — Telegram/Matrix/WhatsApp bridge handlers
  are separate work; this plan makes the attachment infrastructure generic so
  they can plug in.
