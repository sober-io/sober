# 006 --- sober-memory

**Date:** 2026-03-06

---

## BCF (Binary Context Format) --- v1 Subset

BCF is a custom binary format designed specifically for Sober's scoped memory
system. It is not an existing standard — it was created to provide a compact,
self-describing container for memory chunks with built-in support for scope
isolation, optional encryption, and optional compression.

v1 implements BCF incrementally. Encryption and compression flags exist in the
format but are unused in this version.

### Header (16 bytes)

| Offset | Size | Field       | Value / Notes                                      |
|--------|------|-------------|----------------------------------------------------|
| 0      | 4    | Magic       | `0x53 0xD5 0x42 0x45` ("SOBE")                     |
| 4      | 2    | Version     | `1` (u16 LE)                                       |
| 6      | 2    | Flags       | `0x0000` (u16 --- bit 0: encrypted, bit 1: compressed) |
| 8      | 8    | Scope ID    | First 8 bytes of scope UUID (u64 LE)               |
| 16     | 4    | Chunk Count | u32 LE                                             |

### Chunk Table

Array of entries immediately following the header. Each entry:

| Field  | Type | Notes          |
|--------|------|----------------|
| Offset | u64  | Byte offset into the data section |
| Length | u32  | Byte length of chunk data         |
| Type   | u8   | Chunk type discriminant            |

### v1 Chunk Types

| Value | Name         | Encoding | Notes                              |
|-------|--------------|----------|------------------------------------|
| 0     | Fact         | UTF-8    | Extracted knowledge                |
| 1     | Conversation | UTF-8    | Summary or key exchange            |
| 2     | Embedding    | raw f32  | Raw f32 vector (little-endian)     |
| 3--5  | Reserved     | ---      | Skill, Preference, Code (not in v1)|

### Usage

BCF is used for export/backup and offline snapshots. Live memory goes through
Qdrant + PostgreSQL.

### API

```rust
// Writing
BcfWriter::new(scope_id: ScopeId) -> Self
BcfWriter::add_chunk(chunk_type: ChunkType, data: &[u8]) -> Result<()>
BcfWriter::finish(self) -> Result<Vec<u8>>  // produces complete BCF bytes

// Reading
BcfReader::open(data: &[u8]) -> Result<Self>
BcfReader::header(&self) -> &BcfHeader
BcfReader::chunks(&self) -> impl Iterator<Item = Result<Chunk>>
```

---

## Qdrant Integration

- One collection per user: `user_{user_id}`
- Collections created lazily on first interaction
- Vector dimension: depends on embedding model (1536 for text-embedding-3-small
  or similar)
- Distance metric: Cosine
- Hybrid search: dense vectors (cosine) + sparse BM25 via Qdrant's built-in
  sparse vector support

### Payload Fields

| Field              | Type   | Notes                         |
|--------------------|--------|-------------------------------|
| scope_id           | string | Scope UUID                    |
| chunk_type         | u8     | Maps to ChunkType enum        |
| content            | string | Original text content         |
| source_message_id  | string | UUID of originating message   |
| importance         | f32    | Current importance score      |
| created_at         | string | ISO 8601 timestamp            |
| decay_at           | string | ISO 8601 timestamp            |

### API

```rust
MemoryStore::new(qdrant_url: &str) -> Result<Self>
MemoryStore::ensure_collection(user_id: UserId) -> Result<()>
MemoryStore::store(user_id: UserId, chunks: Vec<MemoryChunk>) -> Result<()>
MemoryStore::search(
    user_id: UserId,
    query_vector: Vec<f32>,
    scope_id: ScopeId,
    limit: usize,
) -> Result<Vec<MemoryResult>>
MemoryStore::delete(user_id: UserId, chunk_ids: Vec<Uuid>) -> Result<()>
MemoryStore::prune(
    user_id: UserId,
    before: DateTime<Utc>,
    min_importance: f32,
) -> Result<u64>  // returns count pruned
```

`MemoryChunk` contains: id, vector, content, scope_id, chunk_type, importance,
metadata.

---

## Scoped Retrieval

Context loading follows the principle of least privilege:

- Load only the scopes permitted for the current request.
- Budget-based: retrieve at most N tokens of context (configurable).
- Priority order: session scope (most recent) > user scope (personal facts) >
  system scope (global knowledge).
- `ContextLoader` combines Qdrant search results with recent messages from
  PostgreSQL.

### API

```rust
ContextLoader::new(memory_store: &MemoryStore, db: &PgPool) -> Self
ContextLoader::load(
    user_id: UserId,
    conversation_id: ConversationId,
    query: &str,
    query_vector: Vec<f32>,
    budget: usize,
) -> Result<Context>
```

`Context` struct fields: `facts: Vec<String>`, `recent_messages: Vec<Message>`,
`total_tokens: usize`.

---

## Importance Scoring

- New facts start at importance **1.0**.
- Importance decays over time. Default decay rate: halves every 30 days.
- Facts that are retrieved (used in context) receive an importance boost of
  **+0.2**, capped at **1.0**.
- Pruning removes chunks below a threshold (default **0.1**).

---

## Error Type

```rust
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("Qdrant error: {0}")]
    Qdrant(#[source] /* qdrant error type */),

    #[error("BCF format error: {0}")]
    BcfFormat(String),

    #[error("Scope violation: {0}")]
    ScopeViolation(String),

    #[error("Context budget exceeded: requested {requested}, limit {limit}")]
    BudgetExceeded { requested: usize, limit: usize },
}
```

Maps to `AppError` via `From<MemoryError>`.

---

## Dependencies

| Crate          | Purpose                          |
|----------------|----------------------------------|
| sober-core     | Shared types (UserId, ScopeId, AppError) |
| qdrant-client  | Vector database client           |
| serde          | Serialization                    |
| serde_json     | JSON payload serialization       |
| bincode        | BCF chunk serialization          |
| chrono         | Timestamps, decay calculations   |
| uuid           | Identifiers                      |
| tracing        | Structured logging               |
| sqlx           | PostgreSQL access (for ContextLoader) |
