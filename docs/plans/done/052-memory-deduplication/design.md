# 052 --- Memory Deduplication & Maintenance

**Date:** 2026-04-05

---

## Problem

Every `MemoryStore::store()` call generates a fresh UUIDv7 and upserts unconditionally. No deduplication exists. Duplicates enter through repeated LLM extraction, explicit `remember` calls, and cross-conversation overlap in the same user scope.

Duplicates waste token budget during context loading, degrade recall relevance, and inflate storage.

## Solution

Two layers:

1. **Write-time dedup** — dense cosine similarity check before store. If a match exceeds a threshold, skip the insert and boost the existing memory.
2. **Batch dedup job** — scheduled sweep for duplicates that slipped through (race conditions, threshold changes, pre-existing data).

Decay is computed on-the-fly at read time — no cached importance field, no recalculation job. The single `importance` field is the durable base value (set at creation, incremented by boosts). The existing `prune()` job already computes decay on-the-fly for delete decisions.

## Design

### Collection-per-scope model

Introduces **separate conversation collections** (`conv_{uuid}`). Currently all non-system memories live in `user_{uuid}` with a `scope_id` payload filter.

**Routing** — `collection_for_scope()` becomes a three-way branch:

```rust
fn collection_for_scope(&self, user_id: UserId, scope_id: ScopeId) -> String {
    if scope_id == ScopeId::GLOBAL {
        system_collection_name().to_owned()
    } else if scope_id == ScopeId::from_uuid(*user_id.as_uuid()) {
        user_collection_name(user_id)
    } else {
        conversation_collection_name(scope_id)
    }
}
```

**New helper** in `collections.rs`:

```rust
pub fn conversation_collection_name(scope_id: ScopeId) -> String {
    format!("conv_{}", scope_id.as_uuid().simple())
}

pub const CONVERSATION_COLLECTION_PREFIX: &str = "conv_";
```

**Lifecycle:**
- **Creation** — lazy via existing `create_collection_if_missing()` inside `store()`.
- **Cleanup** — periodic orphan cleanup job (see below). No `sober-memory` dependency in `sober-api`.

**Batch job enumeration** — executors accept an optional `conversation_ids` list in the job payload. The scheduler resolves them from Postgres when creating the job. Keeps the memory crate free of Postgres dependencies.

**Migration** — existing conversation-scoped memories in user collections continue to work. Migration is out of scope; old data decays and gets pruned naturally.

**Context loader** — `ContextLoader::load()` must ensure and search the `conv_{uuid}` collection for conversation scope instead of the user collection. Add `decay_at` to `MemoryHit` (extracted from payload in `scored_point_to_hit()`) so the context loader can compute `decay(importance, elapsed, half_life)` at read time for sorting.

### Importance model: single field, compute decay at read time

The `importance` payload field is the **durable base value** — set at creation, incremented by retrieval boosts and dedup boosts, never decayed in storage.

Decay is computed on-the-fly wherever needed:
- **Context loader** — `decay(importance, elapsed, half_life)` when sorting results for token budget packing.
- **Prune job** — already does this today, no change needed.
- **Batch dedup** — uses stored `importance` directly for tiebreaking (higher importance survives).

No format change to the payload. No migration. No recalculation job.

### Write-time dedup

#### `find_similar` method

```rust
pub async fn find_similar(
    &self,
    user_id: UserId,
    scope_id: ScopeId,
    dense_vector: &[f32],
    threshold: f32,
) -> Result<Option<MemoryHit>, MemoryError>
```

Dense-only cosine query (no RRF, no BM25). `QueryPointsBuilder` with `limit(1)`, `score_threshold(threshold)`, scope filter. Returns raw cosine similarity — directly interpretable for threshold comparison, unlike RRF's rank-based scores.

#### `store_with_dedup` method

```rust
pub async fn store_with_dedup(
    &self,
    user_id: UserId,
    chunk: StoreChunk,
    config: &MemoryConfig,
) -> Result<StoreOutcome, MemoryError>
```

1. If `threshold >= 1.0` → skip dedup, call `store()` directly.
2. Call `find_similar()`.
3. Match found → boost existing via `apply_retrieval_boost()`, return `Deduplicated`.
4. No match → call `store()`, return `Stored`.

#### `StoreOutcome`

```rust
#[derive(Debug)]
pub enum StoreOutcome {
    Stored { point_id: uuid::Uuid },
    Deduplicated { existing_point_id: uuid::Uuid, similarity: f32 },
}
```

### Configuration

Add `dedup_similarity_threshold: f64` to `MemoryConfig` (default: `0.92`).

- `0.92` — conservative, catches near-identical content and close paraphrases
- `0.85` — aggressive, risks false positives
- `1.0` — disables dedup

### Scope isolation

Each scope lives in its own collection (`user_`, `conv_`, `system`). No cross-collection matching. `find_similar()` still filters by `scope_id` within collections for correctness during migration.

### Caller changes

**Ingestion** (`ingestion.rs`) — accept `&MemoryConfig` instead of `half_life_days: u32`, call `store_with_dedup()`.

**RememberTool** (`tools/memory.rs`) — call `store_with_dedup()`, return appropriate message for each `StoreOutcome` variant.

### Race conditions

Actor model serializes per-conversation. Cross-conversation races on the same user scope are rare and benign — caught by the batch dedup job.

### Performance

Dense-only `limit(1)` with `score_threshold` is <5ms. Negligible vs the embedding call (~100-200ms).

---

## Scheduled Job: Batch Dedup

Catches duplicates that slipped through write-time dedup.

**Operation key:** `"memory_dedup"`

```rust
pub struct MemoryDedupExecutor {
    memory_store: Arc<MemoryStore>,
    memory_config: MemoryConfig,
}
```

### Algorithm

Calls `MemoryStore::deduplicate(target, config)`:

```rust
pub enum CollectionTarget {
    User(UserId),
    Conversation(ConversationId),
    System,
}
```

1. Scroll all points in batches of 100 with payload + dense vectors.
2. Group by `scope_id` within each batch.
3. Pairwise cosine similarity in-memory within each scope group. For pairs exceeding threshold:
   - Keep the higher `importance` point (older `created_at` as tiebreaker).
   - Boost survivor's `importance`.
   - Mark the other for deletion.
4. Retain surviving vectors in a running buffer (capped at 1000) for cross-batch comparison.
5. Batch-delete marked points.

**Target resolution from job payload:**
- `owner_id` + no `conversation_ids` → user collection only
- `owner_id` + `conversation_ids` → user + each conversation collection
- No `owner_id` → system collection

**Schedule:** once daily. Registered in `build_executor_registry()`.

---

## Scheduled Job: Orphan Collection Cleanup

Deletes `conv_` collections whose conversation no longer exists in Postgres.

**Operation key:** `"memory_orphan_cleanup"`

```rust
pub struct MemoryOrphanCleanupExecutor<C: ConversationRepo> {
    memory_store: Arc<MemoryStore>,
    conversation_repo: Arc<C>,
}
```

### Algorithm

1. List all Qdrant collections with `conv_` prefix.
2. Extract conversation UUIDs.
3. Batch-check existence against `ConversationRepo`.
4. Delete collections for conversations that no longer exist.

**Schedule:** once daily.

---

## Metrics

| Metric | Type | Labels |
|--------|------|--------|
| `sober_memory_dedup_total` | counter | `outcome`, `chunk_type`, `scope` |
| `sober_memory_dedup_check_duration_seconds` | histogram | `scope` |
| `sober_memory_batch_dedup_runs_total` | counter | — |
| `sober_memory_batch_dedup_duration_seconds` | histogram | — |
| `sober_memory_batch_dedup_merged_total` | counter | — |
| `sober_memory_orphan_cleanup_runs_total` | counter | — |
| `sober_memory_orphan_cleanup_deleted_total` | counter | — |

---

## Files to modify

| File | Change |
|------|--------|
| `sober-core/src/config.rs` | Add `dedup_similarity_threshold` to `MemoryConfig` |
| `sober-memory/src/store/collections.rs` | Add `conversation_collection_name()` + prefix constant |
| `sober-memory/src/store/types.rs` | Add `StoreOutcome`, `CollectionTarget`, `DedupResult` |
| `sober-memory/src/store/mod.rs` | Export new types |
| `sober-memory/src/store/memory_store.rs` | Update `collection_for_scope()`, add `decay_at` to `MemoryHit` + `scored_point_to_hit()`, add `find_similar()`, `store_with_dedup()`, `deduplicate()`, `delete_collection()` |
| `sober-memory/src/loader/context_loader.rs` | Ensure + search conversation collections, compute decayed importance at read time |
| `sober-agent/src/ingestion.rs` | Switch to `store_with_dedup`, accept `&MemoryConfig` |
| `sober-agent/src/tools/memory.rs` | Switch to `store_with_dedup`, handle `StoreOutcome` |
| `sober-agent/src/turn.rs` | Pass `&memory_config` to ingestion |
| `sober-scheduler/src/executors/memory_dedup.rs` | New `MemoryDedupExecutor` |
| `sober-scheduler/src/executors/memory_orphan_cleanup.rs` | New `MemoryOrphanCleanupExecutor` |
| `sober-scheduler/src/executors/mod.rs` | Declare new modules |
| `sober-scheduler/src/main.rs` | Register new executors |
| `sober-memory/tests/store_integration.rs` | Integration tests |

## Test plan

Integration tests (Qdrant required, `#[ignore]`):

1. `store_with_dedup_allows_unique_memories` — distant vectors → both `Stored`
2. `store_with_dedup_detects_duplicate` — identical vector twice → second `Deduplicated`
3. `store_with_dedup_boosts_existing` — duplicate found → existing importance increased
4. `store_with_dedup_respects_scope_isolation` — identical vectors, different scopes → both `Stored`
5. `find_similar_returns_none_for_empty_collection`
6. `deduplicate_merges_similar_points` — identical vectors → one deleted, survivor boosted
7. `deduplicate_preserves_unique_points` — dissimilar vectors → both remain
8. `store_routes_conversation_scope_to_conv_collection` — conversation scope → `conv_` collection
9. `delete_collection_removes_conversation_memories`

Unit tests:
- `MemoryConfig::default().dedup_similarity_threshold == 0.92`
- `conversation_collection_name` produces `conv_` prefix, no hyphens
- `collection_for_scope` routes correctly for all three scope types
