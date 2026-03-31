# 044: Multimodal Content — Implementation Plan

## Step 1: ContentBlock types and domain model changes

**Crate: `sober-core`**

- Define `ContentBlock` enum in `sober-core/src/types/content.rs` with all 5
  variants (Text, Image, File, Audio, Video). Tagged serde serialization.
  Image/File/Audio/Video variants reference `ConversationAttachmentId`.
- Define `ConversationAttachmentId` via `define_id!` macro.
- Define `AttachmentKind` enum: `Image`, `Audio`, `Video`, `Document`.
- Define `ConversationAttachment` domain type with fields: `id`, `blob_key`,
  `kind`, `content_type`, `filename`, `size`, `metadata` (serde_json::Value),
  `conversation_id`, `user_id`, `created_at`.
- Define `CreateConversationAttachment` input type.
- Define `ConversationAttachmentRepo` trait: `create()`, `get_by_id()`,
  `list_by_conversation()`, `delete()`, `delete_orphaned(max_age: Duration)`,
  `find_unreferenced_by_message(conversation_attachment_ids: &[ConversationAttachmentId], conversation_id) -> Vec<ConversationAttachmentId>`.
- Update `Message` domain type: `content: String` -> `content: Vec<ContentBlock>`.
- Update `CreateMessage` input type the same way.
- Add `content` module to `types/mod.rs` re-exports.
- Add helper `Message::text_content() -> String` that extracts joined text blocks.
- Update any `sober-core` code that reads `message.content` as a string.

**Files:** `types/content.rs` (new), `types/domain.rs`, `types/input.rs`,
`types/ids.rs`, `types/enums.rs`, `types/repo.rs`, `types/mod.rs`

**Verify:** `cargo test -p sober-core -q`, `cargo clippy -p sober-core -q`

## Step 2: Image processing and PDF text extraction

**Crate: `sober-workspace`**

- Add `image` crate dependency for resize/re-encode.
- Create `image_processing.rs` module with:
  - `process_image(data: &[u8], content_type: &str) -> Result<ProcessedImage>`
  - `ProcessedImage { data: Vec<u8>, content_type: String, width: u32, height: u32 }`
  - Single variant: max 2048px longest side
  - Re-encode as JPEG (quality 85) or PNG (if input has alpha)
  - GIF passed through without resize
- Add media type validation by magic bytes: `validate_content_type()`.
- Add `derive_attachment_kind(content_type: &str) -> AttachmentKind`.
- Add `pdf-extract` (or similar) crate dependency.
- Create `text_extraction.rs` module:
  - `extract_text(data: &[u8], content_type: &str) -> Result<Option<String>>`
  - Supports `application/pdf` initially.
  - Returns `None` for unsupported types.

**Files:** `image_processing.rs` (new), `text_extraction.rs` (new), `Cargo.toml`

**Verify:** `cargo test -p sober-workspace -q` (unit tests with sample images)

## Step 3: Database migration

- Create migration:
  - `CREATE TYPE attachment_kind AS ENUM ('image', 'audio', 'video', 'document')`
  - `CREATE TABLE conversation_attachments` (schema from design.md)
  - Indexes on `blob_key` and `conversation_id`
  - `ALTER TABLE conversation_messages ALTER COLUMN content TYPE JSONB`
    using `jsonb_build_array(jsonb_build_object('type', 'text', 'text', content))`
  - `CHECK` constraint: `jsonb_typeof(content) = 'array'`
  - Investigate and update any full-text search indexes on `content` column
    (plan #031 conversation search). May need a generated column or function
    index to extract text from JSONB content blocks.
- Implement `PgConversationAttachmentRepo` in `sober-db/src/repos/conversation_attachments.rs`:
  - `create()` — INSERT returning full row
  - `get_by_id()` — SELECT by ID
  - `list_by_conversation()` — SELECT by conversation_id
  - `delete()` — DELETE by ID
  - `delete_orphaned(max_age)` — DELETE rows older than max_age not referenced
    by any message content block
  - `find_unreferenced_by_message()` — given conversation_attachment_ids and conversation_id,
    return those not referenced by any other message in the conversation
- Update `PgMessageRepo`: serialize `Vec<ContentBlock>` as JSONB via
  `sqlx::types::Json`. Update row type to deserialize content as
  `Json<Vec<ContentBlock>>`.
- Run `cargo sqlx prepare`.

**Files:** `migrations/` (new migration), `sober-db/src/repos/` (new + updated)

**Verify:** `cargo test -p sober-db -q` (requires Docker), `cargo sqlx prepare`

## Step 4: Blob GC refactor

**Crate: `sober-workspace`**

- Add `BlobStore::list_keys_batched(batch_size: usize)` returning an async
  iterator/stream of `Vec<(String, SystemTime)>`. Filters by grace period
  on the filesystem side (skip files newer than cutoff).

**Crate: `sober-db`**

- Add `BlobGcRepo` trait in `sober-core/src/types/repo.rs`:
  - `find_unreferenced(keys: &[String]) -> Result<Vec<String>>`
- Implement `PgBlobGcRepo` — single query with `unnest($1)` and `NOT EXISTS`
  against `conversation_attachments`, `plugins`, and `artifacts`.

**Crate: `sober-scheduler`**

- Rewrite `BlobGcExecutor` to use batched walk:
  - `for batch in blob_store.list_keys_batched(100)`
  - `let orphans = gc_repo.find_unreferenced(&batch)`
  - Delete orphans
- Remove `blob_keys_in_use()` from `PluginRepo` and `ArtifactRepo` traits.
- Remove old implementations from `PgPluginRepo` and `PgArtifactRepo`.

**Files:** `sober-workspace/src/blob.rs`, `sober-core/src/types/repo.rs`,
`sober-db/src/repos/blob_gc.rs` (new), `sober-scheduler/src/executors/blob_gc.rs`

**Verify:** `cargo test --workspace -q`

## Step 5: Attachment cleanup job

**Crate: `sober-scheduler`**

- Add `AttachmentCleanupExecutor`:
  - Calls `ConversationAttachmentRepo::delete_orphaned(Duration::from_secs(86400))` (24h)
  - Logs count of deleted rows
- Register as system job `system::attachment_cleanup` with 1h interval.

**Files:** `sober-scheduler/src/executors/attachment_cleanup.rs` (new),
`sober-scheduler/src/system_jobs.rs`

**Verify:** `cargo test -p sober-scheduler -q`

## Step 6: gRPC proto and agent service changes

**Proto + Crate: `sober-agent`**

- Update `agent.proto`: add `ContentBlock` oneof message with `TextBlock`,
  `ImageBlock`, `FileBlock`, `AudioBlock`, `VideoBlock`. Change
  `HandleMessageRequest.content` and `NewMessage.content` to
  `repeated ContentBlock`.
- Update `ConversationActor::InboxMessage::UserMessage` —
  `content: String` -> `content: Vec<ContentBlock>`.
- Update `Agent::handle_message()` to convert proto `ContentBlock` to domain
  `ContentBlock`.
- Update message storage in the actor to use `Vec<ContentBlock>`.
- Update `ingestion.rs` memory extraction pipeline to use `message.text_content()`
  instead of reading `message.content` as a string.
- Add message deletion logic: when a message is deleted, extract conversation_attachment_ids
  from content blocks, call `ConversationAttachmentRepo::find_unreferenced_by_message()`,
  delete unreferenced attachment rows.

**Files:** `proto/sober/agent/v1/agent.proto`, `sober-agent/src/conversation.rs`,
`sober-agent/src/service.rs`

**Verify:** `cargo build -p sober-agent -q`, `cargo test -p sober-agent -q`

## Step 7: LLM content resolution

**Crate: `sober-llm`**

- Add `ModelCapabilities` struct with `vision` flag (other flags stubbed).
- Add `MessageContent` enum (Text | Blocks) with custom `Serialize` —
  plain string for text-only, array for multimodal.
- Add `LlmContentBlock` enum: `Text`, `ImageBase64`.
- Update `Message::content` from `Option<String>` to `MessageContent`.
- Update `Message::user()`, `Message::system()` constructors.
- Streaming parser (`MessageDelta`) stays `Option<String>` — LLM providers
  only stream text. No changes needed. Document the asymmetry: outbound is
  multimodal (`MessageContent`), inbound streaming is text-only.
- Update `OpenAiCompatibleEngine` serialization for outbound `MessageContent`.

**Crate: `sober-agent`**

- At the start of history assembly in `to_llm_messages()`, batch-load all
  `ConversationAttachmentId`s referenced across the conversation's messages
  in one query. Resolve from the in-memory map during content block processing
  to avoid N+1 queries.
- Add `resolve_content_blocks()` function in `history.rs` that maps
  `Vec<ContentBlock>` -> `Vec<LlmContentBlock>` using `BlobStore` +
  pre-loaded attachment map + capabilities.
- Image resolution: load attachment metadata, read blob, base64 encode.
  If model lacks vision, emit `[Image: {alt}]` placeholder.
- File: use `extracted_text` from attachment metadata or `[File: {filename}]`.
- Audio/Video: use transcript from metadata or text placeholder.

**Crate: `sober-agent` (turn.rs)**

- Update `run_turn()`: after stream completes, wrap `content_buffer` into
  `vec![ContentBlock::Text { text: content_buffer }]` before creating
  `CreateMessage`. The `content_buffer: String` accumulation is unchanged —
  only the final wrapping is new.

**Files:** `sober-llm/src/types.rs`, `sober-llm/src/client.rs`,
`sober-agent/src/history.rs`, `sober-agent/src/turn.rs`

**Verify:** `cargo test -p sober-llm -q`, `cargo test -p sober-agent -q`

## Step 8: Upload and serve API endpoints

**Crate: `sober-api`**

- Add `POST /api/v1/conversations/{id}/attachments` handler:
  - Accept `multipart/form-data`
  - Validate file size (configurable max, default 25 MB)
  - Validate content type via magic bytes
  - Derive `kind` from validated content type
  - If image: run image processing pipeline (resize + re-encode)
  - If PDF: extract text, store in metadata
  - Store bytes via `BlobStore`
  - Create `conversation_attachments` row
  - Return attachment receipt
- Add `GET /api/v1/attachments/{id}/content` handler:
  - Load attachment row for `content_type` and `filename`
  - Stream blob with `Content-Type` and `Content-Disposition` headers
  - `Cache-Control: public, max-age=31536000, immutable`
- Update message list/get endpoints to include hydrated `attachments` map
  in the response. Batch-load attachment rows for all conversation_attachment_ids
  referenced in the returned messages' content blocks.
- Add routes to router.
- Add `ConversationAttachmentRepo` to `AppState`.

**Files:** `sober-api/src/routes/attachments.rs` (new),
`sober-api/src/routes/mod.rs`, `sober-api/src/state.rs`

**Verify:** `cargo test -p sober-api -q`, manual test with curl

## Step 9: WebSocket protocol changes

**Crate: `sober-api`**

- Update `ClientWsMessage::ChatMessage` — `content: String` ->
  `content: Vec<ContentBlock>`.
- Update `ServerWsMessage::ChatNewMessage` — same change. Include hydrated
  `attachments` map for any attachment-bearing content blocks.
- Update WebSocket handler to pass `Vec<ContentBlock>` through to gRPC.
- Update `SubscribeConversationUpdates` event handling for new content format.
- Text deltas stay as `String` (only final message is content blocks).

**Files:** `sober-api/src/routes/ws.rs`

**Verify:** `cargo build -p sober-api -q`

## Step 10: Frontend TypeScript types

- Add `ContentBlock` discriminated union type.
- Add `ConversationAttachment` interface.
- Update `Message.content` from `string` to `ContentBlock[]`.
- Update `ClientWsMessage` `chat.message` content type.
- Update `ServerWsMessage` `chat.new_message` content type.
- Add `getMessageText(msg: Message): string` helper.
- **Audit all `message.content` string usages** — project-wide search for
  `.content` on Message types. Every string usage must migrate to
  `getMessageText()` or iterate `ContentBlock[]`. Key locations: conversation
  list sidebar (preview text), search results, any component reading
  `message.content` directly.
- Add `UploadState` type for the upload state machine.
- Add attachment service in `$lib/services/attachments.ts`:
  - `uploadAttachment(conversationId, file): Promise<ConversationAttachment>`
  - `getAttachmentUrl(id): string`

**Files:** `frontend/src/lib/types/index.ts`,
`frontend/src/lib/services/attachments.ts` (new)

**Verify:** `pnpm check`

## Step 11: Upload state management

- Create `$lib/stores/uploads.svelte.ts`:
  - `attachments` state: `SvelteMap<string, AttachmentState>`
  - `addFiles(conversationId, files: FileList)` — validate, create preview
    URLs, start uploads
  - `removeAttachment(id: string)` — cancel if uploading, remove
  - `buildContentBlocks(text: string): ContentBlock[]` — assemble text +
    ready attachments
  - `clear()` — reset after send
  - `hasUploading` derived — true if any attachment is still uploading
  - `hasAttachments` derived — true if any attachments present

**Files:** `frontend/src/lib/stores/uploads.svelte.ts` (new)

**Verify:** `pnpm check`

## Step 12: ChatInput.svelte — attachment support

- Add attach button (paperclip icon) that opens file picker
- Add drag-and-drop zone on the chat area (visual feedback on dragover)
- Add clipboard paste handler for images
- Add `AttachmentPreview.svelte` component — renders preview strip above input
  - Image attachments: thumbnail preview (local blob URL)
  - File attachments: icon + filename + size
  - Uploading state: spinner overlay
  - Remove button (x) on each
- Disable send while `hasUploading` is true (in-progress uploads only).
  Failed uploads show error indicator, are removable, but don't block send.
- On send: call `buildContentBlocks(text)` instead of sending plain string
- Update chat page streaming model: add `streamingText: string` field to local
  `ChatMsg` type. Delta handler appends to `streamingText` (simple string
  concatenation, same as before). On `Done` event, convert to
  `content: [{ type: 'text', text: streamingText }]`.

**Files:** `frontend/src/lib/components/ChatInput.svelte`,
`frontend/src/lib/components/AttachmentPreview.svelte` (new)

**Verify:** `pnpm check`, `pnpm test --silent`, manual test

## Step 13: Content block rendering

- Create `ContentBlockRenderer.svelte` — switch on `block.type`, render:
  - `text` -> existing markdown renderer
  - `image` -> `ImageBlock.svelte`
  - `file` -> `FileBlock.svelte`
  - `audio` -> placeholder text (future)
  - `video` -> placeholder text (future)
- Create `ImageBlock.svelte`:
  - `<img loading="lazy">` with `src="/api/v1/attachments/{id}/content"`
  - Max width constrained, aspect ratio preserved
  - Click to expand (basic: open in new tab; lightbox deferred)
  - Alt text from content block
- Create `FileBlock.svelte`:
  - Card with file type icon, filename, size
  - Download link to `/api/v1/attachments/{id}/content`
- Update `ChatMessage.svelte` to iterate `message.content` blocks and render
  via `ContentBlockRenderer` instead of rendering `message.content` as text.
- Update conversation list preview to use `getMessageText()`.

**Files:** `frontend/src/lib/components/ContentBlockRenderer.svelte` (new),
`frontend/src/lib/components/ImageBlock.svelte` (new),
`frontend/src/lib/components/FileBlock.svelte` (new),
`frontend/src/lib/components/ChatMessage.svelte`

**Verify:** `pnpm check`, `pnpm test --silent`, manual test

## Step 14: Integration testing and Docker rebuild

- Write integration test: upload image -> send message with content blocks ->
  verify stored in DB as JSONB -> verify attachment accessible.
- Write integration test: text-only message still works (backward compatibility).
- Write integration test: blob GC batched walk deletes unreferenced blobs,
  preserves attachment-referenced blobs.
- Write integration test: conversation deletion cascades to attachments.
- Rebuild Docker images with new dependencies (image processing, PDF extraction).
- End-to-end manual test: upload image in browser -> agent responds with
  vision-based analysis.

**Verify:** `cargo test --workspace -q`, `pnpm build && pnpm check && pnpm test --silent`,
`docker compose up -d --build --quiet-pull | tail -15`
