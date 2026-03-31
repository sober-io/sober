# #037: Remove BCF Module

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the unused BCF (Binary Context Format) binary serialization module. Keep `ChunkType` in `sober-memory` where it belongs — just move it out of the doomed `bcf/` directory.

**Architecture:** BCF writer/reader have zero production callers. `ChunkType` is actively used across 4 crates but is a memory-domain concept — stays in `sober-memory`. Move it to `store/types.rs` (colocated with `StoreChunk`/`MemoryHit` that use it), delete `bcf/`, clean up error variants and docs.

**Tech Stack:** Rust (cargo workspace)

---

### Task 1: Move ChunkType to store/types.rs and delete BCF module

**Files:**
- Modify: `backend/crates/sober-memory/src/store/types.rs`
- Modify: `backend/crates/sober-memory/src/lib.rs`
- Modify: `backend/crates/sober-memory/src/error.rs`
- Modify: `backend/crates/sober-memory/src/store/memory_store.rs`
- Modify: `backend/crates/sober-memory/src/loader/context_loader.rs`
- Delete: `backend/crates/sober-memory/src/bcf/` (entire directory)

- [ ] **Step 1: Add ChunkType to store/types.rs**

In `backend/crates/sober-memory/src/store/types.rs`, remove the `use crate::bcf::ChunkType;` import and add the enum definition directly. Place it before `StoreChunk`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sober_core::{MessageId, ScopeId};

/// Memory chunk type discriminant.
///
/// Categorises knowledge extracted from conversations before storage
/// in the vector database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum ChunkType {
    /// Extracted knowledge fact.
    Fact = 0,
    /// User preference or personal setting.
    Preference = 1,
    /// Decision or choice made, with rationale.
    Decision = 2,
    /// Soul layer data (internal, used by sober-mind).
    Soul = 3,
}

impl std::fmt::Display for ChunkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Fact => "fact",
            Self::Preference => "preference",
            Self::Decision => "decision",
            Self::Soul => "soul",
        };
        f.write_str(s)
    }
}

impl From<ChunkType> for u8 {
    fn from(ct: ChunkType) -> Self {
        ct as u8
    }
}
```

Note: `TryFrom<u8>` is dropped — its only caller was the BCF reader we're deleting.

- [ ] **Step 2: Update store/memory_store.rs import**

In `backend/crates/sober-memory/src/store/memory_store.rs`, change:

```rust
// Remove: use crate::bcf::ChunkType;
```

`ChunkType` is now in the same `store` module — import via:

```rust
use super::types::ChunkType;
```

Or if it's already importing from `super::types::{MemoryHit, StoreChunk, StoreQuery}`, add `ChunkType` to that import.

- [ ] **Step 3: Update loader/context_loader.rs import**

In `backend/crates/sober-memory/src/loader/context_loader.rs`, change:

```rust
// Remove: use crate::bcf::ChunkType;
// Add:
use crate::store::types::ChunkType;
```

- [ ] **Step 4: Update lib.rs — remove bcf, keep ChunkType re-export**

In `backend/crates/sober-memory/src/lib.rs`:

```rust
//! Vector storage, memory pruning, and scoped retrieval for the Sober system.
//!
//! This crate is the sole interface to Qdrant — no other crate should access
//! the vector database directly.

pub mod error;
pub mod loader;
pub mod scoring;
pub mod store;

pub use error::MemoryError;
pub use loader::{ContextLoader, LoadRequest, LoadedContext};
pub use scoring::{boost, decay, should_prune};
pub use store::{ChunkType, MemoryHit, MemoryStore, StoreChunk, StoreQuery};
```

Key: `ChunkType` is now re-exported from `store` instead of `bcf`. External crates using `sober_memory::ChunkType` or `sober_memory::bcf::ChunkType` — the former still works, the latter will break (which is what we want).

- [ ] **Step 5: Update store/mod.rs to re-export ChunkType**

Check `backend/crates/sober-memory/src/store/mod.rs` — ensure it re-exports `ChunkType` from `types`:

```rust
pub use types::{ChunkType, MemoryHit, StoreChunk, StoreQuery};
```

- [ ] **Step 6: Remove BCF-specific error variants**

In `backend/crates/sober-memory/src/error.rs`, remove:

```rust
    /// BCF format validation failure.
    #[error("bcf format error: {0}")]
    BcfFormat(String),

    /// Unknown chunk type discriminant.
    #[error("invalid chunk type: {0}")]
    InvalidChunkType(u8),
```

- [ ] **Step 7: Delete the bcf directory**

```bash
rm -rf backend/crates/sober-memory/src/bcf/
```

- [ ] **Step 8: Verify sober-memory compiles**

Run: `cargo build -q -p sober-memory`
Expected: success

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "refactor(memory): move ChunkType to store/types, delete BCF module"
```

---

### Task 2: Fix external crate imports

**Files:**
- Modify: `backend/crates/sober-agent/src/extraction.rs`
- Modify: `backend/crates/sober-agent/src/tools/memory.rs`
- Modify: `backend/crates/sober-agent/src/agent.rs`
- Modify: `backend/crates/sober-mind/src/layers.rs`
- Modify: `backend/crates/sober-plugin/src/backends/memory.rs`
- Modify: `backend/crates/sober-memory/tests/store_integration.rs`

- [ ] **Step 1: Fix sober-agent imports**

`backend/crates/sober-agent/src/extraction.rs`:
```rust
// Change: use sober_memory::bcf::ChunkType;
// To:
use sober_memory::ChunkType;
```

`backend/crates/sober-agent/src/tools/memory.rs`:
```rust
// Change: use sober_memory::bcf::ChunkType;
// To:
use sober_memory::ChunkType;
```

`backend/crates/sober-agent/src/agent.rs` — uses fully-qualified `sober_memory::ChunkType::Fact` in test code. These still work since `sober_memory::ChunkType` is re-exported. No change needed unless the test also imports via `sober_memory::bcf::ChunkType`.

- [ ] **Step 2: Verify sober-mind and sober-plugin**

`backend/crates/sober-mind/src/layers.rs` uses `sober_memory::{ChunkType, ...}` — this still works. No change needed.

`backend/crates/sober-plugin/src/backends/memory.rs` uses `sober_memory::ChunkType::Fact` — still works. No change needed.

- [ ] **Step 3: Check integration tests**

`backend/crates/sober-memory/tests/store_integration.rs` — verify it doesn't import from `bcf::`. Update if needed.

- [ ] **Step 4: Verify full workspace compiles and tests pass**

Run: `cargo build -q --workspace && cargo clippy -q --workspace -- -D warnings && cargo test --workspace -q`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor(agent): update ChunkType imports after BCF removal"
```

---

### Task 3: Update ARCHITECTURE.md and mdBook docs

**Files:**
- Modify: `ARCHITECTURE.md`
- Modify: `docs/book/src/architecture/memory-system.md`
- Modify: `docs/book/src/architecture/crates.md`
- Modify: `docs/book/src/architecture/overview.md`
- Modify: `docs/book/src/architecture/agent-mind.md`
- Modify: `docs/book/src/architecture/security.md`
- Modify: `docs/book/src/introduction.md`
- Modify: `docs/book/src/user-guide/memory.md`
- Modify: `docs/book/src/getting-started/configuration.md`

- [ ] **Step 1: Update ARCHITECTURE.md**

Remove the "Binary Context Format (BCF)" subsection (lines 113-119). The section currently reads:

```markdown
### Binary Context Format (BCF)

Compact binary format: 28-byte header (magic `SÕBE`, version, flags, scope UUID,
chunk count) → chunk table → zstd-compressed + optionally AES-256-GCM encrypted
chunks → embedded HNSW vector index footer.

Chunk types: `Fact`, `Preference`, `Decision`, `Soul`.
```

Delete this entire subsection. The `### Scoped Memory` section follows directly after `## Memory & Context System`.

Also update the `sober-memory` crate map row. Change:

```
| `sober-memory` | Vector storage, binary context format, pruning, scoped retrieval |
```

To:

```
| `sober-memory` | Vector storage, memory pruning, scoped retrieval |
```

- [ ] **Step 2: Update memory-system.md**

`docs/book/src/architecture/memory-system.md` — the heaviest BCF content. Remove the entire "Binary Context Format (BCF)" section (header format, chunk table, zero-copy reader description). Rewrite the opening paragraph to describe the memory system as Qdrant-backed only. Remove the "HNSW index footer" pruning reference (line 142) and the "BCF container" scoped memory reference (line 66). Replace with Qdrant scope filtering description.

- [ ] **Step 3: Update crates.md**

`docs/book/src/architecture/crates.md` — change sober-memory row:

```
| `sober-memory` | Vector storage, Binary Context Format (BCF), pruning, scoped retrieval |
```

To:

```
| `sober-memory` | Vector storage, memory pruning, scoped retrieval |
```

- [ ] **Step 4: Update overview.md**

`docs/book/src/architecture/overview.md` — replace "scoped BCF memory containers" with "scoped Qdrant vector collections":

```
strict context isolation (scoped BCF memory containers, no cross-user leakage)
```

To:

```
strict context isolation (scoped Qdrant vector collections, no cross-user leakage)
```

- [ ] **Step 5: Update agent-mind.md**

`docs/book/src/architecture/agent-mind.md` — two references:

Line 67: change "Per-user and per-group BCF `Soul` chunks" to "Per-user and per-group `Soul` chunks stored in Qdrant".

Line 89: change "add and modify `Soul` chunks in BCF containers" to "add and modify `Soul` chunks in Qdrant".

- [ ] **Step 6: Update security.md**

`docs/book/src/architecture/security.md` — rewrite scope isolation paragraph (line 132). Replace BCF file/header description with Qdrant payload filtering:

```
Memory scopes are enforced at the query level. Each memory chunk carries a scope UUID
in its Qdrant payload. The context loading pipeline applies scope filters on every query,
ensuring chunks from different user scopes are never mixed. A bug that incorrectly
constructs a query filter would still produce empty results rather than silently leaking
data, because Qdrant's payload filtering is applied server-side.
```

- [ ] **Step 7: Update introduction.md**

`docs/book/src/introduction.md` — two references:

Line 47-48: replace "compact Binary Context Format (BCF)" with "scoped vector collections in Qdrant".

Line 82: replace "separate scoped BCF containers" with "separate scoped Qdrant collections".

- [ ] **Step 8: Update user-guide/memory.md**

`docs/book/src/user-guide/memory.md` — remove the "## Binary Context Format (BCF)" section entirely (line 52+). This was a user-facing explanation of a format they never interact with.

- [ ] **Step 9: Update getting-started/configuration.md**

`docs/book/src/getting-started/configuration.md` — two references:

Line 258: change "master key for BCF encryption" to "master key for envelope encryption" (the key is still used for sober-crypto, just not BCF specifically).

Line 291: change "BCF containers are not encrypted at rest" to "secrets are not encrypted at rest" or similar — the encryption key is used for secret storage, not BCF.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "docs: remove all BCF references from user-facing documentation"
```

---

### Task 4: Close plan

**Files:**
- Delete: `docs/plans/pending/037-bcf-expansion/design.md`
- Move: `docs/plans/pending/037-bcf-expansion/` → `docs/plans/done/037-bcf-removal/`

- [ ] **Step 1: Delete old design.md**

```bash
rm docs/plans/pending/037-bcf-expansion/design.md
```

- [ ] **Step 2: Move plan to done with renamed folder**

```bash
mkdir -p docs/plans/done/037-bcf-removal
cp docs/plans/pending/037-bcf-expansion/plan.md docs/plans/done/037-bcf-removal/plan.md
rm -rf docs/plans/pending/037-bcf-expansion
```

- [ ] **Step 3: Run final verification**

```bash
cargo clippy -q --workspace -- -D warnings && cargo test --workspace -q
```
Expected: clean clippy, all tests pass

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: close plan #037 as BCF removal"
```
