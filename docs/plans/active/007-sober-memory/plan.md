# 007 --- sober-memory: Implementation Plan

**Date:** 2026-03-06

---

## Steps

1. **Add dependencies to Cargo.toml.** Add `qdrant-client`, `bincode`, `serde`,
   `serde_json`, `chrono`, `uuid`, `tracing`, `sqlx`, and `thiserror` to the
   `sober-memory` crate. Depend on `sober-core` for shared types.

2. **Create module structure.**
   ```
   sober-memory/src/
       lib.rs
       error.rs
       bcf/
           mod.rs
           types.rs
           writer.rs
           reader.rs
       store.rs
       context.rs
       scoring.rs
   ```

3. **Implement `error.rs`.** Define `MemoryError` enum with variants: `Qdrant`,
   `BcfFormat`, `ScopeViolation`, `BudgetExceeded`. Implement `From<MemoryError>`
   for `AppError`.

4. **Implement `bcf/types.rs`.** Define `BcfHeader` (28 bytes: Magic + Version +
   Flags + ScopeID as full 128-bit UUID + ChunkCount), `ChunkType` enum (Fact,
   Conversation, Embedding, Soul), `ChunkEntry` (offset, length, type), and
   `Chunk` (type + owned data).

5. **Implement `bcf/writer.rs`.** `BcfWriter` builds BCF bytes incrementally:
   collects chunks in memory, then `finish()` writes header + chunk table +
   chunk data in one pass.

6. **Implement `bcf/reader.rs`.** `BcfReader` parses BCF bytes: validates magic
   and version, reads chunk table, provides an iterator over chunks. Returns
   `MemoryError::BcfFormat` on invalid input.

7. **Implement `store.rs`.** `MemoryStore` wraps `qdrant-client`: collection
   management (`ensure_collection`), `store`, `search`, `delete`, and `prune`.
   Collections use cosine distance. Prune deletes points where
   `importance < min_importance` and `created_at < before`.

8. **Implement `context.rs`.** `ContextLoader` combines Qdrant search results
   with recent messages from PostgreSQL. Respects a token budget. Prioritizes
   session scope, then user scope, then system scope.

9. **Implement `scoring.rs`.** Importance decay calculation (exponential decay
   with configurable half-life). Boost function for retrieved chunks. Pure
   functions, no side effects.

10. **Write unit tests.**
    - BCF roundtrip: write N chunks of each type, read them back, verify data
      matches exactly.
    - Header validation: reject invalid magic bytes, reject unsupported versions.
    - Chunk type serialization: verify u8 discriminants map correctly.
    - Importance scoring math: decay(1.0, 30 days) is approximately 0.5; boost
      is capped at 1.0; decay(x, 0 days) equals x.

11. **Write integration tests (require running Qdrant).**
    - Store chunks, search by vector, verify results returned.
    - Prune by importance threshold and timestamp, verify count.
    - Collection creation is idempotent (calling `ensure_collection` twice
      succeeds).

12. **Run lints and tests.**
    ```
    cargo clippy -p sober-memory -- -D warnings
    cargo test -p sober-memory
    ```

---

## Acceptance Criteria

- BCF roundtrip tests pass: write N chunks, read them back, data matches.
- BCF rejects invalid magic bytes and unsupported versions with
  `MemoryError::BcfFormat`.
- Qdrant integration tests pass: store, search, and prune cycle works correctly.
- Context loading respects token budget --- never returns more tokens than
  requested.
- Importance decay math is correct: `decay(1.0, 30_days) ~= 0.5` (property
  test).
- `cargo clippy -p sober-memory -- -D warnings` is clean.
- No direct Qdrant access from outside this crate --- `sober-memory` is the
  sole Qdrant interface.
