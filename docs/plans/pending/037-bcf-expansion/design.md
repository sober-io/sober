# BCF Format Expansion — Compression, Encryption & HNSW Index

## Overview

Expand the Binary Context Format (BCF) with three capabilities currently reserved in the header but not implemented: zstd compression, AES-256-GCM encryption, and an embedded HNSW vector index footer for offline similarity search.

## Current State

### BCF Format (v1)

Layout: `[Header: 28 bytes][Chunk Table: N × 13 bytes][Data section]`

- **Header** (`bcf/types.rs`): magic `SÕBE`, version (u16), flags (u16), scope UUID, chunk count (u32)
- **Flags**: bit 0 = encrypted, bit 1 = compressed — defined but always written as `0u16`
- **Writer** (`bcf/writer.rs`): collects chunks in-memory, `finish()` serializes to bytes — no compression or encryption
- **Reader** (`bcf/reader.rs`): zero-copy design, `BcfChunk<'a>` borrows from source — no decompression or decryption
- **7 chunk types**: Fact, Conversation, Embedding, Preference, Skill, Code, Soul

### Dependencies Available

- `aes-gcm = "0.10"` — already in `sober-crypto/Cargo.toml`
- `zstd` — NOT in workspace, needs adding
- `qdrant-client = "1.17.0"` — in `sober-memory/Cargo.toml`
- Embedding vectors stored as `Vec<f32>` in Qdrant with dense + sparse (BM25) indexing

### BCF Usage

Export/snapshot format only. Live memory uses Qdrant + PostgreSQL. BCF containers are used for:
- Memory export/backup
- Offline snapshots
- Potential future use: portable agent memory for replica spawning

## Design

### Version Bump: BCF v2

Bump `BCF_VERSION` to `2`. The reader must handle both v1 and v2:
- v1: current format (no compression, no encryption, no index)
- v2: compression/encryption/index supported, controlled by flags

### Feature Flags (u16)

Expand the existing flags field:

| Bit | Name | Description |
|-----|------|-------------|
| 0 | `ENCRYPTED` | Data section is AES-256-GCM encrypted |
| 1 | `COMPRESSED` | Data section is zstd compressed |
| 2 | `HAS_INDEX` | HNSW vector index footer appended after data |
| 3-15 | Reserved | Must be 0 |

Flags compose: a container can be compressed AND encrypted AND have an index. Processing order:
- **Write**: serialize → compress → encrypt → append index
- **Read**: strip index → decrypt → decompress → parse

### A. Zstd Compression

Compress the data section (chunk payloads) only. Header and chunk table remain uncompressed so the reader can parse metadata without decompressing.

**Layout (compressed):**
```
[Header: 28 bytes][Chunk Table: N × 13 bytes][zstd(Data section)]
```

**Implementation:**
- Add `zstd` crate to `sober-memory/Cargo.toml`
- `BcfWriter::finish()`: if compression enabled, zstd-compress the concatenated chunk data before writing
- `BcfReader::parse()`: if `COMPRESSED` flag set, decompress data section before resolving chunk offsets
- Chunk table offsets refer to positions within the *uncompressed* data — the reader decompresses first, then applies offsets
- Compression level: use zstd default (level 3) — good balance of speed and ratio for mixed text/binary content

**Considerations:**
- Zero-copy reading is no longer possible when compressed — the reader must own the decompressed buffer
- `BcfReader` gains a `data_owned: Option<Vec<u8>>` field for the decompressed case
- `BcfChunk<'a>` lifetime still borrows from the reader (which owns the buffer)

### B. AES-256-GCM Encryption

Encrypt the data section (and index footer if present). Header and chunk table remain plaintext — chunk types and counts are visible without the key, but content is protected.

**Layout (encrypted):**
```
[Header: 28 bytes][Chunk Table: N × 13 bytes][Nonce: 12 bytes][Encrypted Data][Tag: 16 bytes]
```

**Implementation:**
- Reuse `aes-gcm` from `sober-crypto` — add `sober-crypto` as a dependency of `sober-memory` (or extract shared encryption primitives to `sober-core`)
- Key source: the scope's encryption key, derived from the master key via the existing envelope encryption in `sober-crypto`
- `BcfWriter::finish()`: if encryption enabled, generate random 12-byte nonce, encrypt data section with AES-256-GCM, append tag
- `BcfReader::parse()`: if `ENCRYPTED` flag set, extract nonce and tag, decrypt data section
- AAD (additional authenticated data): header bytes (28) + chunk table bytes — ensures header/table integrity without encrypting them

**Key management:**
- Writer accepts `Option<&[u8; 32]>` encryption key
- Reader accepts `Option<&[u8; 32]>` decryption key
- If `ENCRYPTED` flag set but no key provided, return `BcfError::EncryptionKeyRequired`

**Processing order with compression:**
- Write: data → compress → encrypt (compress first for better ratios on plaintext)
- Read: decrypt → decompress

### C. HNSW Vector Index Footer

Append an embedded HNSW index after the data section for offline similarity search without Qdrant. This enables portable, self-contained memory containers.

**Layout (with index):**
```
[Header][Chunk Table][Data Section][HNSW Index][Index Size: 8 bytes]
```

The last 8 bytes of the file contain the index size (u64 LE), allowing the reader to locate the index by reading from the end.

**HNSW Index Contents:**

The index is a serialized structure containing:
1. **Index metadata**: vector dimensionality (u32), distance metric (u8), entry point ID
2. **Vectors**: all `Embedding` chunk vectors (f32 arrays), referenced by chunk index
3. **Graph layers**: HNSW navigation graph (neighbors lists per layer)

**Implementation options:**
1. **`instant-distance`** crate — pure Rust HNSW implementation, serializable, no unsafe. Lightweight.
2. **`hnsw_rs`** crate — more feature-rich, supports serialization
3. **Custom minimal implementation** — just the graph structure, since we only need offline search

**Recommended: `instant-distance`** — it's pure Rust, has serde support, and the API is simple:
```rust
let hnsw = HnswMap::new(points, values, config);
let neighbors = hnsw.search(&query_point, &mut search_state);
```

**Integration with BCF:**
- `BcfWriter`: if `HAS_INDEX` enabled, collect all `Embedding` chunks, build HNSW index, serialize and append
- `BcfReader`: if `HAS_INDEX` flag set, read index size from last 8 bytes, deserialize HNSW index
- New method: `BcfReader::search(query_vector, k) -> Vec<(usize, f32)>` — returns chunk indices + distances
- The index maps vector positions to chunk table indices, so search results can be resolved to chunk content

**Considerations:**
- Index building happens at write time — acceptable for export/snapshot use case
- Index covers only `Embedding` chunk type — other chunks don't have vectors
- If the BCF is also compressed+encrypted, the index sits outside the encrypted envelope (after encryption) so the reader can check `HAS_INDEX` without decrypting. The index itself should be encrypted separately if the `ENCRYPTED` flag is set.

### API Changes

**BcfWriter:**
```rust
impl BcfWriter {
    pub fn new(scope_id: Uuid) -> Self;
    pub fn with_compression(self) -> Self;
    pub fn with_encryption(self, key: &[u8; 32]) -> Self;
    pub fn with_index(self) -> Self;
    pub fn add_chunk(&mut self, chunk_type: ChunkType, data: Vec<u8>);
    pub fn finish(self) -> Result<Vec<u8>, BcfError>;
}
```

**BcfReader:**
```rust
impl<'a> BcfReader<'a> {
    pub fn parse(data: &'a [u8], key: Option<&[u8; 32]>) -> Result<Self, BcfError>;
    pub fn header(&self) -> &BcfHeader;
    pub fn chunks(&self) -> impl Iterator<Item = BcfChunk<'_>>;
    pub fn search(&self, query: &[f32], k: usize) -> Option<Vec<(usize, f32)>>;
}
```

### Error Types

New `BcfError` variants:
- `CompressionError(String)` — zstd compress/decompress failure
- `EncryptionKeyRequired` — encrypted BCF without key
- `DecryptionFailed` — wrong key or corrupted data
- `IndexError(String)` — HNSW build/search failure
- `UnsupportedVersion(u16)` — unknown BCF version

### Testing

- Roundtrip tests: write compressed → read decompressed, content matches
- Roundtrip tests: write encrypted → read decrypted with correct key
- Roundtrip tests: write with index → search returns correct chunks
- Combined: compressed + encrypted + indexed roundtrip
- Error cases: wrong key, truncated data, v1 reader on v2 data (graceful error)
- Property tests: random chunk data survives compress/encrypt roundtrip
- Benchmark: compression ratio and speed for typical memory chunks

## Scope

**In scope:**
- BCF v2 format with compression, encryption, and HNSW index
- Backwards-compatible reader (handles v1 and v2)
- Builder pattern API for writer options
- `zstd` dependency addition
- HNSW crate integration
- Unit and property-based tests

**Out of scope:**
- Changing live memory storage (still Qdrant)
- BCF-based memory loading at runtime (BCF remains export/snapshot format)
- Streaming read/write (entire BCF fits in memory)
- Index updates (rebuild on each write — acceptable for snapshots)
