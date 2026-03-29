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
        blob_key: String,
        llm_blob_key: Option<String>,  // LLM-optimized variant
        media_type: String,            // image/png, image/jpeg, image/webp, image/gif
        alt: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
    },
    File {
        blob_key: String,
        filename: String,
        media_type: String,
        size: u64,
        extracted_text: Option<String>, // PDF text extraction for LLM context
    },
    Audio {
        blob_key: String,
        media_type: String,
        duration_secs: Option<f64>,
        transcript: Option<String>,    // STT result for LLM context
    },
    Video {
        blob_key: String,
        media_type: String,
        duration_secs: Option<f64>,
        thumbnail_key: Option<String>,
        transcript: Option<String>,
    },
}
```

Existing messages migrate via:

```sql
ALTER TABLE conversation_messages
    ALTER COLUMN content TYPE JSONB
    USING jsonb_build_array(
        jsonb_build_object('type', 'text', 'text', content)
    );
```

## Storage Abstraction

A `ContentStore` trait abstracts over storage backends. Local filesystem wraps
the existing `BlobStore`. S3-compatible backends are a future implementation.

```rust
trait ContentStore: Send + Sync {
    fn store(&self, data: &[u8], media_type: &str)
        -> impl Future<Output = Result<BlobRef>>;
    fn read(&self, key: &str)
        -> impl Future<Output = Result<Bytes>>;
    fn url(&self, key: &str, expiry: Duration)
        -> impl Future<Output = Result<Option<String>>>;
    fn exists(&self, key: &str)
        -> impl Future<Output = Result<bool>>;
    fn delete(&self, key: &str)
        -> impl Future<Output = Result<()>>;
}

struct BlobRef {
    key: String,
    size: u64,
}
```

- **Local (`FsBlobStore`)**: wraps existing `BlobStore`. `url()` returns `None` —
  LLM adapter falls back to base64 encoding. Frontend serves blobs via
  `GET /api/v1/blobs/{key}`.
- **S3 (future)**: `url()` returns presigned URLs with configurable expiry.
  LLM adapter sends URLs directly. Frontend can use presigned URLs too.

### Image Processing Pipeline

Uploaded images are resized into variants. Originals are not stored by default.

| Variant | Max dimension | Purpose |
|---------|---------------|---------|
| Display | 1200px | Frontend rendering |
| LLM | ≤2048px | Sent to vision-capable models |

Images are re-encoded during resize, which strips embedded payloads.

## Upload API

### POST /api/v1/uploads

Multipart form upload. Requires authentication.

1. Validate file size (configurable max, default 25 MB)
2. Validate media type against allowlist (magic bytes, not just extension)
3. If image → run processing pipeline (resize variants)
4. If PDF → extract text
5. Store via `ContentStore`
6. Return upload receipt

Response:

```json
{
  "data": {
    "blob_key": "a1b2c3d4...",
    "media_type": "image/jpeg",
    "size": 204800,
    "content_type": "image",
    "width": 1200,
    "height": 900,
    "llm_blob_key": "e5f6g7..."
  }
}
```

Allowed media types:

- Images: `image/jpeg`, `image/png`, `image/webp`, `image/gif`
- Files: `application/pdf`, `text/plain`, `text/csv`, `text/markdown`,
  `application/json`, `application/xml`
- Audio: `audio/mpeg`, `audio/wav`, `audio/webm`, `audio/ogg`
- Video: `video/mp4`, `video/webm`

### GET /api/v1/blobs/{blob_key}

Serves blob content. Requires auth — user must belong to a conversation
referencing this blob_key.

- Local backend: streams bytes with `Content-Type` header
- S3 backend: 302 redirect to presigned URL
- `Cache-Control: public, max-age=31536000, immutable` (content-addressed)

## WebSocket Protocol Changes

```typescript
// Before
{ type: "chat.message", conversation_id: "...", content: "text" }

// After
{ type: "chat.message", conversation_id: "...", content: [
    { type: "text", text: "What's in this?" },
    { type: "image", blob_key: "a1b2c3..." }
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
    file_input: bool,
    audio_input: bool,
    video_input: bool,
    image_generation: bool,
    audio_output: bool,
    max_image_size: Option<u64>,
    max_images_per_request: Option<u32>,
    supported_image_types: Vec<String>,
}
```

### Content Resolution

`to_llm_messages()` resolves each `ContentBlock` based on provider capabilities
and storage backend:

| Block Type | Provider Supports It | Provider Doesn't |
|------------|---------------------|-------------------|
| Text | Pass through | n/a |
| Image | URL (if store supports) or base64 | `[Image: {alt}]` |
| File | Native file API (future) | Send `extracted_text` or `[File: {name}]` |
| Audio | Native audio (future) | Send `transcript` or `[Audio: {duration}s]` |
| Video | Native video (future) | Send `transcript` or `[Video: {duration}s]` |

### LLM Message Content

```rust
enum MessageContent {
    Text(String),                   // → "content": "hello"
    Blocks(Vec<LlmContentBlock>),   // → "content": [{...}, {...}]
}
```

Smart serialization: text-only messages emit a plain string (backward compatible
with all providers). Content array only emitted when media blocks are present.

### Agent-Produced Rich Output

Tools that generate media store results in `ContentStore` and return
`ContentBlock` variants mixed into the assistant message:

- `generate_image` tool → `ContentBlock::Image`
- `create_artifact` tool → `ContentBlock::File`
- `text_to_speech` tool → `ContentBlock::Audio` (future)

Generation provider routing (DALL-E, ElevenLabs, etc.) is a separate future
design concern — the content block system doesn't depend on it.

## Frontend Changes

### Chat Input

- **Attach button (+)** — opens file picker filtered by allowed types
- **Drag & drop** — files dropped anywhere on the chat area
- **Clipboard paste** — Ctrl+V images from clipboard
- **Preview strip** — thumbnails above the input, removable with ×
- **Upload on attach** — files upload immediately via `POST /uploads`, not on send
- **Send** — assembles content blocks from text + uploaded blob_keys

### Upload State Machine

Each attachment tracks: `uploading` → `ready` (or `failed`). Send button
disabled while any attachment is still uploading.

### Content Block Rendering

Each `ContentBlock` type renders differently in `ChatMessage.svelte`:

- **Text** → Markdown rendering (existing)
- **Image** → Inline `<img>` with click to expand
- **File** → Card with icon, filename, size, download link
- **Audio** → Native `<audio>` player (future)
- **Video** → Native `<video>` player (future)

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
  | { type: 'image'; blob_key: string; llm_blob_key?: string;
      media_type: string; alt?: string; width?: number; height?: number }
  | { type: 'file'; blob_key: string; filename: string;
      media_type: string; size: number; extracted_text?: string }
  | { type: 'audio'; blob_key: string; media_type: string;
      duration_secs?: number; transcript?: string }
  | { type: 'video'; blob_key: string; media_type: string;
      duration_secs?: number; thumbnail_key?: string; transcript?: string };
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
| Unauthorized blob access | `GET /blobs/{key}` requires auth + conversation membership referencing the blob. S3 presigned URLs are short-lived (5–15 min). |
| Storage exhaustion | Per-user upload quota (configurable). Content-addressed dedup. Image variants replace originals. |
| XSS via filename/alt | Filenames sanitized on upload. All user strings escaped in rendering. `Content-Disposition: attachment` for downloads. |
| SSRF via image URL | Not applicable — all content uploaded as bytes, never fetched by URL. |

## Scope

### Build Now

- `ContentBlock` enum with all 5 types
- `ContentStore` trait + local filesystem impl
- Image processing pipeline (resize variants)
- Upload endpoint (`POST /api/v1/uploads`) with validation
- Blob read endpoint (`GET /api/v1/blobs/{key}`)
- Database migration (TEXT → JSONB)
- gRPC proto updates (`ContentBlock` oneof)
- Agent pipeline (content resolution in `to_llm_messages()`)
- LLM `MessageContent` with smart serialization
- `ModelCapabilities` with vision flag
- Frontend: attach button, drop zone, clipboard paste, preview strip
- Content block rendering: `ImageBlock`, `FileBlock` components
- PDF text extraction on upload

### Design Now, Build Later

- S3 `ContentStore` implementation
- Audio/Video block processing and rendering
- Audio transcription (STT) pipeline
- Image generation provider routing
- TTS provider routing
- `AudioBlock` / `VideoBlock` renderer components
- Image lightbox (fullscreen viewer)
- Upload progress indicators
