# 044: Multimodal Content

## Overview

Replace text-only message content with a content block model supporting images,
files, audio, and video. Both directions: users send rich media to the agent,
and the agent produces rich media in responses.

First implementation ships image and file support end-to-end. Audio and video
types exist in the model for forward compatibility but processing/rendering is
deferred.

## Content Block Model

Messages change from `content: String` to `content: Vec<ContentBlock>` across
the entire pipeline — domain types, database (JSONB), gRPC proto, WebSocket
protocol, and frontend.

```rust
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        conversation_attachment_id: ConversationAttachmentId,
        alt: Option<String>,
    },
    File {
        conversation_attachment_id: ConversationAttachmentId,
    },
    Audio {
        conversation_attachment_id: ConversationAttachmentId,
    },
    Video {
        conversation_attachment_id: ConversationAttachmentId,
    },
}
```

Content blocks reference conversation attachments by ID. The attachment row
holds all metadata (content type, dimensions, extracted text). Content blocks
are lightweight pointers.

Existing messages migrate via:

```sql
ALTER TABLE conversation_messages
    ALTER COLUMN content TYPE JSONB
    USING jsonb_build_array(
        jsonb_build_object('type', 'text', 'text', content)
    );
```

## Attachment Model

Uploaded files are tracked in `conversation_attachments` — a thin metadata table
that pairs a blob key with its content type, kind, and type-specific metadata.

```sql
CREATE TYPE attachment_kind AS ENUM ('image', 'audio', 'video', 'document');

CREATE TABLE conversation_attachments (
    id              UUID PRIMARY KEY,
    blob_key        TEXT NOT NULL,
    kind            attachment_kind NOT NULL,
    content_type    TEXT NOT NULL,
    filename        TEXT NOT NULL,
    size            BIGINT NOT NULL,
    metadata        JSONB NOT NULL DEFAULT '{}',
    conversation_id UUID REFERENCES conversations(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conversation_attachments_blob_key
    ON conversation_attachments(blob_key);
CREATE INDEX idx_conversation_attachments_conversation_id
    ON conversation_attachments(conversation_id);
```

### Kind and Metadata

`kind` is server-derived from the validated `content_type` (magic bytes, not
extension):

| content_type prefix | kind | metadata fields |
|---------------------|------|-----------------|
| `image/*` | `image` | `{width, height}` |
| `audio/*` | `audio` | `{duration_secs}` |
| `video/*` | `video` | `{duration_secs}` |
| everything else | `document` | `{extracted_text}` (PDF), `{}` (others) |

Type-specific fields live in `metadata` JSONB — avoids a wide table with mostly
NULL columns that grows a column every time a new content type is added.

### Image Processing

Uploaded images are resized to a single variant (max 2048px longest side) and
re-encoded (JPEG quality 85, PNG if alpha). This variant serves both frontend
rendering and LLM vision input. Re-encoding strips embedded payloads.

Originals are not stored. One blob per image.

### Relationship to BlobStore

Attachments use the existing `BlobStore` for byte storage. No new storage
abstraction (no `ContentStore` trait, no sidecar files):

```
conversation_attachments (DB)     BlobStore (filesystem)
┌──────────────────────────┐     ┌─────────────────────┐
│ id: uuid                 │     │ /blobs/ab/abc123...  │
│ blob_key: "abc123..."  ──┼────▶│ (actual bytes)       │
│ content_type: "image/jpg"│     └─────────────────────┘
│ filename: "photo.jpg"    │
│ kind: image              │
│ metadata: {w:800,h:600}  │
└──────────────────────────┘
```

## Upload API

### POST /api/v1/conversations/{id}/attachments

Multipart form upload. Requires authentication + conversation membership.

1. Validate file size (configurable max, default 25 MB)
2. Validate content type against allowlist (magic bytes, not extension)
3. Derive `kind` from validated content type
4. If image: resize (max 2048px), re-encode
5. If PDF: extract text for LLM context
6. Store bytes in BlobStore
7. Create `conversation_attachments` row
8. Return attachment receipt

Response:

```json
{
  "data": {
    "id": "550e8400-...",
    "blob_key": "a1b2c3d4...",
    "kind": "image",
    "content_type": "image/jpeg",
    "filename": "photo.jpg",
    "size": 204800,
    "metadata": { "width": 1200, "height": 900 }
  }
}
```

Allowed content types:

- Images: `image/jpeg`, `image/png`, `image/webp`, `image/gif`
- Documents: `application/pdf`, `text/plain`, `text/csv`, `text/markdown`,
  `application/json`, `application/xml`
- Audio: `audio/mpeg`, `audio/wav`, `audio/webm`, `audio/ogg`
- Video: `video/mp4`, `video/webm`

### GET /api/v1/attachments/{id}/content

Serves attachment content. No auth check — attachment IDs are UUIDv4 (2^122
possible values), effectively unguessable. Knowing the ID requires conversation
membership (received via message content blocks). This is appropriate for a
personal assistant; multi-tenant SaaS would need explicit auth.

- Loads attachment row for `content_type` and `filename`
- Streams blob bytes with `Content-Type` header
- `Content-Disposition: attachment; filename="..."` for download
- `Cache-Control: public, max-age=31536000, immutable` (content-addressed)

## Attachment Hydration in Message Responses

Message API responses include hydrated attachment data alongside content blocks,
so the frontend has everything it needs to render file cards, image dimensions,
etc. without separate fetches.

```json
{
  "data": {
    "id": "msg-001",
    "role": "user",
    "content": [
      { "type": "text", "text": "What's in this?" },
      { "type": "image", "conversation_attachment_id": "550e8400-..." }
    ],
    "attachments": {
      "550e8400-...": {
        "id": "550e8400-...",
        "kind": "image",
        "content_type": "image/jpeg",
        "filename": "photo.jpg",
        "size": 204800,
        "metadata": { "width": 1200, "height": 900 }
      }
    }
  }
}
```

The `attachments` map is keyed by attachment ID for O(1) lookup during
rendering. It's populated by a batch query when loading messages — no N+1.

## Attachment Lifecycle and Cleanup

Three cleanup paths, all feeding into blob GC:

| Trigger | What happens | Blob cleanup |
|---------|-------------|-------------|
| Abandoned upload (>24h, no message ref) | Periodic job deletes attachment row | Blob GC sweeps |
| Message deleted | App logic deletes unreferenced attachment rows | Blob GC sweeps |
| Conversation deleted | `ON DELETE CASCADE` deletes attachment rows | Blob GC sweeps |

### Abandoned Upload Detection

A scheduler job runs periodically and deletes attachment rows where:
- `created_at` is older than 24 hours
- No `conversation_messages.content` JSONB block references the `conversation_attachment_id`

### Message Deletion Cleanup

When a message is deleted, extract `conversation_attachment_id` values from its content
blocks. For each, check if any other message in the conversation still
references it. If not, delete the attachment row.

## Blob GC Refactor

The current blob GC loads all referenced keys into a `HashSet` in memory, then
iterates all blobs on disk. This doesn't scale with thousands of attachments.

**New approach:** walk the filesystem in batches of 100, query the DB per batch:

```rust
for batch in blob_store.list_keys_batched(100) {
    let orphans = gc_repo.find_unreferenced(&batch).await?;
    for key in orphans {
        blob_store.delete(&key).await?;
    }
}
```

Per-batch query:

```sql
SELECT key FROM unnest($1::text[]) AS key
WHERE NOT EXISTS (SELECT 1 FROM conversation_attachments WHERE blob_key = key)
  AND NOT EXISTS (SELECT 1 FROM plugins WHERE config->>'wasm_blob_key' = key
                                           OR config->>'manifest_blob_key' = key)
  AND NOT EXISTS (SELECT 1 FROM artifacts WHERE blob_key = key AND state != 'archived');
```

Grace period filtering happens on the filesystem side — skip files newer than
the grace period before adding to batch. Index on
`conversation_attachments(blob_key)` ensures fast lookups.

This replaces the `blob_keys_in_use()` pattern entirely. The `PluginRepo` and
`ArtifactRepo` traits no longer need that method — all reference checking is
consolidated in a single `BlobGcRepo::find_unreferenced()` query.

## Impact on Existing Pipelines

### Memory Extraction

The agent's `ingestion.rs` extracts text from messages for embedding into
vector memory. It currently reads `message.content` as a `String`. After this
change, it must use `message.text_content()` to extract joined text blocks.

### Conversation Search

Plan #031 added conversation search, which may index message content with
full-text search. The TEXT→JSONB migration changes the column type, which may
break existing search indexes or queries. The migration step must investigate
and update any full-text search configuration to extract text from the JSONB
content blocks (e.g., via a generated column or function index).

## WebSocket Protocol Changes

```typescript
// Before
{ type: "chat.message", conversation_id: "...", content: "text" }

// After
{ type: "chat.message", conversation_id: "...", content: [
    { type: "text", text: "What's in this?" },
    { type: "image", conversation_attachment_id: "550e8400-..." }
]}
```

`chat.new_message` server event carries the same `content: ContentBlock[]` format.

## gRPC Proto Changes

```protobuf
message ContentBlock {
  oneof block {
    TextBlock text = 1;
    ImageBlock image = 2;
    FileBlock file = 3;
    AudioBlock audio = 4;
    VideoBlock video = 5;
  }
}

message TextBlock { string text = 1; }
message ImageBlock { string conversation_attachment_id = 1; optional string alt = 2; }
message FileBlock { string conversation_attachment_id = 1; }
message AudioBlock { string conversation_attachment_id = 1; }
message VideoBlock { string conversation_attachment_id = 1; }

message HandleMessageRequest {
  string user_id = 1;
  string conversation_id = 2;
  repeated ContentBlock content = 3;
}

message NewMessage {
  string message_id = 1;
  string role = 2;
  repeated ContentBlock content = 3;
  string source = 4;
  optional string user_id = 5;
}
```

## LLM Provider Abstraction

### Model Capabilities

```rust
struct ModelCapabilities {
    vision: bool,
    // Future: file_input, audio_input, video_input, image_generation, audio_output
}
```

### Content Resolution

`to_llm_messages()` resolves each `ContentBlock` based on provider capabilities.
Attachment metadata is loaded to build the LLM representation:

| Block Type | Provider Supports It | Provider Doesn't |
|------------|---------------------|-------------------|
| Text | Pass through | n/a |
| Image | base64 from BlobStore | `[Image: {alt}]` |
| File | Send `extracted_text` from metadata | `[File: {filename}]` |
| Audio | Native audio (future) | Send `transcript` or `[Audio]` |
| Video | Native video (future) | Send `transcript` or `[Video]` |

### LLM Message Content

```rust
enum MessageContent {
    Text(String),                   // -> "content": "hello"
    Blocks(Vec<LlmContentBlock>),   // -> "content": [{...}, {...}]
}
```

Smart serialization: text-only messages emit a plain string (backward compatible
with all providers). Content array only emitted when media blocks are present.

### Streaming Asymmetry

**Outbound** messages to the LLM use `MessageContent` (multimodal — text blocks,
base64 images, etc.). **Inbound** streaming responses are always text deltas —
LLM providers only stream text tokens. The streaming parser (`MessageDelta`)
stays `Option<String>`, unchanged.

When a streamed response completes, the accumulated text buffer is wrapped:
`vec![ContentBlock::Text { text: content_buffer }]`. The streaming layer does
not need multimodal awareness.

### Agent-Produced Rich Output

Tools that generate media store results in BlobStore, create attachment rows,
and return `ContentBlock` variants mixed into the assistant message:

- `generate_image` tool -> `ContentBlock::Image`
- `create_artifact` tool -> `ContentBlock::File`

Generation provider routing (DALL-E, etc.) is a separate future design concern.

## Frontend Changes

### Chat Input

- **Attach button (+)** — opens file picker filtered by allowed types
- **Drag & drop** — files dropped anywhere on the chat area
- **Clipboard paste** — Ctrl+V images from clipboard
- **Preview strip** — thumbnails above the input, removable with x
- **Upload on attach** — files upload immediately via POST, not on send
- **Send** — assembles content blocks from text + uploaded attachment IDs

### Upload State Machine

Each attachment tracks: `uploading` -> `ready` (or `failed`). Send button
disabled while any attachment is still uploading (in-progress). Failed uploads
show an error indicator and are removable, but do not block sending — the user
can send text plus any successful attachments.

### Streaming Model

During LLM streaming, the frontend maintains a `streamingText: string` field
on the local message representation. Text deltas append to this string (simple
concatenation, same as before). When the `Done` event arrives, the streaming
text is converted to `content: [{ type: 'text', text: streamingText }]`. The
frontend never maintains a `ContentBlock[]` during streaming — the conversion
happens once at completion.

### Content Block Rendering

Each `ContentBlock` type renders differently in `ChatMessage.svelte`:

- **Text** -> Markdown rendering (existing)
- **Image** -> Inline `<img loading="lazy">` with click to expand
- **File** -> Card with icon, filename, size, download link
- **Audio** -> Native `<audio>` player (future)
- **Video** -> Native `<video>` player (future)

### New Components

| Component | Purpose |
|-----------|---------|
| `AttachmentPreview.svelte` | Thumbnail strip above chat input |
| `ContentBlockRenderer.svelte` | Dispatches to type-specific renderers |
| `ImageBlock.svelte` | Inline image with click to expand |
| `FileBlock.svelte` | File card with download link |
| `uploads.svelte.ts` | Upload state machine, file validation |

### TypeScript Types

```typescript
type ContentBlock =
  | { type: 'text'; text: string }
  | { type: 'image'; conversation_attachment_id: string; alt?: string }
  | { type: 'file'; conversation_attachment_id: string }
  | { type: 'audio'; conversation_attachment_id: string }
  | { type: 'video'; conversation_attachment_id: string };

interface ConversationAttachment {
  id: string;
  kind: 'image' | 'audio' | 'video' | 'document';
  content_type: string;
  filename: string;
  size: number;
  metadata: Record<string, unknown>;
}
```

Messages include a hydrated attachments map:

```typescript
interface Message {
  // ... existing fields ...
  content: ContentBlock[];
  attachments?: Record<string, ConversationAttachment>; // keyed by ID
}
```

Helper for backward-compatible text extraction:

```typescript
function getMessageText(msg: Message): string {
  return msg.content
    .filter(b => b.type === 'text')
    .map(b => b.text)
    .join('\n');
}
```

## Security

| Threat | Mitigation |
|--------|-----------|
| Malicious file upload | Strict allowlist validated by magic bytes. Max file size enforced server-side. Images re-encoded during resize — strips embedded payloads. |
| Unauthorized blob access | ConversationAttachment IDs and blob keys are unguessable (UUID + SHA-256). Access requires knowing the ID, which requires conversation membership. |
| Storage exhaustion | Per-user upload quota (configurable). Content-addressed dedup. Abandoned upload cleanup (24h TTL). |
| XSS via filename/alt | Filenames sanitized on upload. All user strings escaped in rendering. `Content-Disposition: attachment` for downloads. |
| SSRF via image URL | Not applicable — all content uploaded as bytes, never fetched by URL. |

## Scope

### Build Now

- `ContentBlock` enum with all 5 types (referencing attachment IDs)
- `conversation_attachments` table + `ConversationAttachment` domain type
- Image processing (single variant, max 2048px resize + re-encode)
- Upload endpoint (`POST /api/v1/conversations/{id}/attachments`)
- Attachment content endpoint (`GET /api/v1/attachments/{id}/content`)
- Database migration (TEXT -> JSONB for message content)
- gRPC proto updates (`ContentBlock` oneof)
- Agent pipeline (content resolution in `to_llm_messages()`)
- LLM `MessageContent` with smart serialization
- `ModelCapabilities` with vision flag
- Blob GC refactor (batched filesystem walk, consolidated DB query)
- Attachment cleanup (abandoned uploads, message deletion, conversation cascade)
- Frontend: attach button, drop zone, clipboard paste, preview strip
- Content block rendering: `ImageBlock`, `FileBlock` components
- PDF text extraction on upload

### Recommended PR Split

- **PR 1 (backend foundation):** Steps 1-5 — types, processing, migration, GC refactor, cleanup job
- **PR 2 (backend pipeline):** Steps 6-9 — gRPC, LLM, API endpoints, WebSocket
- **PR 3 (frontend):** Steps 10-14 — types, upload state, input, rendering, integration tests

### Design Now, Build Later

- S3-compatible storage backend
- Audio/Video block processing and rendering
- Audio transcription (STT) pipeline
- Image generation provider routing
- TTS provider routing
- `AudioBlock` / `VideoBlock` renderer components
- Image lightbox (fullscreen viewer)
- Upload progress indicators
