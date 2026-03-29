# 044: Multimodal Content — Implementation Plan

## Step 1: ContentBlock types and domain model changes

**Crate: `sober-core`**

- Define `ContentBlock` enum in `sober-core/src/types/content.rs` with all 5
  variants (Text, Image, File, Audio, Video). Tagged serde serialization.
- Update `Message` domain type: `content: String` → `content: Vec<ContentBlock>`.
- Update `CreateMessage` input type the same way.
- Add `content` module to `types/mod.rs` re-exports.
- Add helper `Message::text_content() -> String` that extracts joined text blocks.
- Update any `sober-core` code that reads `message.content` as a string.

**Files:** `types/content.rs` (new), `types/domain.rs`, `types/input.rs`, `types/mod.rs`

**Verify:** `cargo test -p sober-core -q`, `cargo clippy -p sober-core -q`

## Step 2: ContentStore trait and local filesystem implementation

**Crate: `sober-workspace`**

- Define `ContentStore` trait in `sober-workspace/src/content_store.rs` with
  `store()`, `read()`, `url()`, `exists()`, `delete()` methods. Return `BlobRef`
  from `store()`.
- Implement `ContentStore` for existing `BlobStore` as `FsBlobStore` (or impl
  directly on `BlobStore`). `url()` returns `Ok(None)`. `store()` delegates to
  `BlobStore::store()` (ignore `media_type` for local — just store bytes).
- Re-export trait and impl from `lib.rs`.

**Files:** `content_store.rs` (new), `lib.rs`

**Verify:** `cargo test -p sober-workspace -q`

## Step 3: Image processing pipeline

**Crate: `sober-workspace`** (or new module in workspace)

- Add `image` crate dependency for resize/re-encode.
- Create `image_processing.rs` module with:
  - `process_image(data: &[u8], media_type: &str) -> Result<ImageVariants>`
  - `ImageVariants { display: ProcessedImage, llm: ProcessedImage }`
  - `ProcessedImage { data: Vec<u8>, media_type: String, width: u32, height: u32 }`
  - Display variant: max 1200px longest side
  - LLM variant: max 2048px longest side
  - Both re-encoded as JPEG (quality 85) or PNG (if input has alpha)
- Add media type validation by magic bytes (not extension): `validate_media_type()`.

**Files:** `image_processing.rs` (new), `Cargo.toml`

**Verify:** `cargo test -p sober-workspace -q` (unit tests with sample images)

## Step 4: PDF text extraction

**Crate: `sober-workspace`**

- Add `pdf-extract` (or similar) crate dependency.
- Create `text_extraction.rs` module:
  - `extract_text(data: &[u8], media_type: &str) -> Result<Option<String>>`
  - Supports `application/pdf` initially, extensible for other types.
  - Returns `None` for unsupported types.

**Files:** `text_extraction.rs` (new), `Cargo.toml`

**Verify:** `cargo test -p sober-workspace -q`

## Step 5: Database migration

- Create migration: `ALTER COLUMN content TYPE JSONB USING jsonb_build_array(...)`.
- Add `CHECK` constraint: `jsonb_typeof(content) = 'array'`.
- Update `sober-db` row types to deserialize `content` as `Vec<ContentBlock>`
  (sqlx `Json<Vec<ContentBlock>>`).
- Update `PgMessageRepo::create()` to serialize `Vec<ContentBlock>` as JSONB.
- Update `PgMessageRepo` queries that read `content`.
- Run `cargo sqlx prepare` to update offline query data.

**Files:** `migrations/` (new migration), `sober-db/src/repos/messages.rs`,
`sober-db/src/repos/` (any other repos reading messages)

**Verify:** `cargo test -p sober-db -q` (requires Docker), `cargo sqlx prepare`

## Step 6: gRPC proto and agent service changes

**Proto + Crate: `sober-agent`**

- Update `agent.proto`: add `ContentBlock` oneof message, change
  `HandleMessageRequest.content` and `NewMessage.content` to
  `repeated ContentBlock`.
- Update `ConversationActor::InboxMessage::UserMessage` —
  `content: String` → `content: Vec<ContentBlock>`.
- Update `Agent::handle_message()` to convert proto `ContentBlock` to domain
  `ContentBlock`.
- Update message storage in the actor to use `Vec<ContentBlock>`.

**Files:** `proto/sober/agent/v1/agent.proto`, `sober-agent/src/conversation.rs`,
`sober-agent/src/service.rs`

**Verify:** `cargo build -p sober-agent -q`, `cargo test -p sober-agent -q`

## Step 7: LLM content resolution

**Crate: `sober-llm`**

- Add `ModelCapabilities` struct with `vision` flag (other flags stubbed).
- Add `MessageContent` enum (Text | Blocks) with custom `Serialize` —
  plain string for text-only, array for multimodal.
- Add `LlmContentBlock` enum: `Text`, `ImageUrl`, `ImageBase64`.
- Update `Message::content` from `Option<String>` to `MessageContent`.
- Update `Message::user()`, `Message::system()` constructors.
- Update streaming parser to handle the new content type.
- Update `OpenAiCompatibleEngine` serialization.

**Crate: `sober-agent`**

- Add `resolve_content_blocks()` function in `history.rs` that maps
  `Vec<ContentBlock>` → `Vec<LlmContentBlock>` using `ContentStore` + capabilities.
- Pass `ContentStore` and `ModelCapabilities` into `to_llm_messages()`.
- Image resolution: try `store.url()` first, fall back to base64.
- File/Audio/Video: use `extracted_text`/`transcript` or text placeholder.

**Files:** `sober-llm/src/types.rs`, `sober-llm/src/client.rs`,
`sober-llm/src/streaming.rs`, `sober-agent/src/history.rs`

**Verify:** `cargo test -p sober-llm -q`, `cargo test -p sober-agent -q`

## Step 8: Upload and blob API endpoints

**Crate: `sober-api`**

- Add `POST /api/v1/uploads` handler:
  - Accept `multipart/form-data`
  - Validate file size (configurable max, default 25 MB)
  - Validate media type via magic bytes
  - If image: run image processing pipeline, store variants
  - If PDF: extract text
  - Store via `ContentStore`
  - Return upload receipt (blob_key, media_type, size, dimensions, llm_blob_key)
- Add `GET /api/v1/blobs/{blob_key}` handler:
  - Auth check: user must belong to conversation referencing this blob
  - Stream blob with `Content-Type` header
  - `Cache-Control: public, max-age=31536000, immutable`
- Add upload routes to router.
- Add `ContentStore` to `AppState`.

**Files:** `sober-api/src/routes/uploads.rs` (new), `sober-api/src/routes/blobs.rs` (new),
`sober-api/src/routes/mod.rs`, `sober-api/src/state.rs`

**Verify:** `cargo test -p sober-api -q`, manual test with curl

## Step 9: WebSocket protocol changes

**Crate: `sober-api`**

- Update `ClientWsMessage::ChatMessage` — `content: String` →
  `content: Vec<ContentBlock>`.
- Update `ServerWsMessage::ChatNewMessage` — same change.
- Update WebSocket handler to pass `Vec<ContentBlock>` through to gRPC.
- Update `SubscribeConversationUpdates` event handling for new content format.
- Update `ChatDelta` — text deltas stay as `String` (only final message
  is content blocks).

**Files:** `sober-api/src/routes/ws.rs`

**Verify:** `cargo build -p sober-api -q`

## Step 10: Frontend TypeScript types

- Add `ContentBlock` discriminated union type.
- Update `Message.content` from `string` to `ContentBlock[]`.
- Update `ClientWsMessage` `chat.message` content type.
- Update `ServerWsMessage` `chat.new_message` content type.
- Add `getMessageText(msg: Message): string` helper.
- Add `UploadState` type for the upload state machine.
- Add upload service in `$lib/services/uploads.ts`.

**Files:** `frontend/src/lib/types/index.ts`, `frontend/src/lib/services/uploads.ts` (new)

**Verify:** `pnpm check`

## Step 11: Upload state management

- Create `$lib/stores/uploads.svelte.ts`:
  - `attachments` state: `SvelteMap<string, AttachmentState>`
  - `addFiles(files: FileList)` — validate, create preview URLs, start uploads
  - `removeAttachment(id: string)` — cancel if uploading, remove
  - `buildContentBlocks(text: string): ContentBlock[]` — assemble text + ready attachments
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
  - Remove button (×) on each
- Disable send while `hasUploading` is true
- On send: call `buildContentBlocks(text)` instead of sending plain string

**Files:** `frontend/src/lib/components/ChatInput.svelte`,
`frontend/src/lib/components/AttachmentPreview.svelte` (new)

**Verify:** `pnpm check`, `pnpm test --silent`, manual test

## Step 13: Content block rendering

- Create `ContentBlockRenderer.svelte` — switch on `block.type`, render:
  - `text` → existing markdown renderer
  - `image` → `ImageBlock.svelte`
  - `file` → `FileBlock.svelte`
  - `audio` → placeholder text (future)
  - `video` → placeholder text (future)
- Create `ImageBlock.svelte`:
  - `<img>` with `src="/api/v1/blobs/{blob_key}"`
  - Max width constrained, aspect ratio preserved
  - Click to expand (basic: open in new tab; lightbox deferred)
  - Alt text from content block
- Create `FileBlock.svelte`:
  - Card with file type icon, filename, size
  - Download link to `/api/v1/blobs/{blob_key}`
- Update `ChatMessage.svelte` to iterate `message.content` blocks and render
  via `ContentBlockRenderer` instead of rendering `message.content` as text.
- Update conversation list preview to use `getMessageText()`.

**Files:** `frontend/src/lib/components/ContentBlockRenderer.svelte` (new),
`frontend/src/lib/components/ImageBlock.svelte` (new),
`frontend/src/lib/components/FileBlock.svelte` (new),
`frontend/src/lib/components/ChatMessage.svelte`

**Verify:** `pnpm check`, `pnpm test --silent`, manual test

## Step 14: Integration testing and Docker rebuild

- Write integration test: upload image → send message with content blocks →
  verify stored in DB as JSONB → verify blob accessible.
- Write integration test: text-only message still works (backward compatibility).
- Rebuild Docker images with new dependencies (image processing, PDF extraction).
- End-to-end manual test: upload image in browser → agent responds with
  vision-based analysis.

**Verify:** `cargo test --workspace -q`, `pnpm build && pnpm check && pnpm test --silent`,
`docker compose up -d --build --quiet-pull | tail -15`
