# Memory Deduplication & Maintenance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add write-time and batch deduplication to the memory store, route conversation-scoped memories to dedicated Qdrant collections, and add scheduled maintenance jobs for batch dedup and orphan cleanup.

**Architecture:** Two-layer dedup: a dense cosine similarity check before every `store()` call catches near-identical content at write time; a scheduled batch job sweeps each collection for duplicates that slipped through race conditions. Conversation-scoped memories move from the user collection (filtered by `scope_id`) to dedicated `conv_{uuid}` collections, matching the existing `user_` and `system` collection model. Decay is computed on-the-fly at read time using the durable `importance` base value.

**Tech Stack:** Rust, Qdrant (qdrant-client), tokio, sober-core config, sober-scheduler executor framework.

**Design doc:** `docs/plans/pending/052-memory-deduplication/design.md`

---

### Task 1: Config — Add `dedup_similarity_threshold` to `MemoryConfig`

**Files:**
- Modify: `backend/crates/sober-core/src/config.rs:394-414`

- [ ] **Step 1: Write unit test for default value**

In `backend/crates/sober-core/src/config.rs`, add inside the existing `#[cfg(test)]` module (or create one at the end of the file):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_config_default_dedup_threshold() {
        let config = MemoryConfig::default();
        assert!(
            (config.dedup_similarity_threshold - 0.92).abs() < f64::EPSILON,
            "default dedup threshold should be 0.92, got {}",
            config.dedup_similarity_threshold
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sober-core -q -- memory_config_default_dedup_threshold`
Expected: compilation error — `dedup_similarity_threshold` doesn't exist yet.

- [ ] **Step 3: Add constant and field**

In `backend/crates/sober-core/src/config.rs`, add the constant near the other memory defaults (after line 73):

```rust
/// Default dedup similarity threshold (cosine, 0.0–1.0).
pub const DEFAULT_MEMORY_DEDUP_SIMILARITY_THRESHOLD: f64 = 0.92;
```

Add the field to `MemoryConfig` (after `prune_threshold`):

```rust
/// Cosine similarity threshold for write-time deduplication.
/// Memories with similarity >= this value are considered duplicates.
/// Set to 1.0 to disable dedup.
pub dedup_similarity_threshold: f64,
```

Add to `Default` impl:

```rust
dedup_similarity_threshold: DEFAULT_MEMORY_DEDUP_SIMILARITY_THRESHOLD,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p sober-core -q -- memory_config_default_dedup_threshold`
Expected: PASS

- [ ] **Step 5: Run full crate tests and clippy**

Run: `cargo test -p sober-core -q && cargo clippy -p sober-core -q -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-core/src/config.rs
git commit -m "feat(core): add dedup_similarity_threshold to MemoryConfig"
```

---

### Task 2: Collection helpers — Add `conversation_collection_name`

**Files:**
- Modify: `backend/crates/sober-memory/src/store/collections.rs`
- Modify: `backend/crates/sober-memory/src/store/mod.rs:8`

- [ ] **Step 1: Write unit tests**

Add to `collections.rs` inside the existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn conversation_collection_contains_no_hyphens() {
    let scope_id = sober_core::ScopeId::from_uuid(uuid::Uuid::new_v4());
    let name = conversation_collection_name(scope_id);
    assert!(!name.contains('-'));
}

#[test]
fn conversation_collection_starts_with_prefix() {
    let scope_id = sober_core::ScopeId::from_uuid(uuid::Uuid::new_v4());
    let name = conversation_collection_name(scope_id);
    assert!(name.starts_with(CONVERSATION_COLLECTION_PREFIX));
}

#[test]
fn conversation_collection_is_deterministic() {
    let scope_id = sober_core::ScopeId::from_uuid(uuid::Uuid::new_v4());
    let a = conversation_collection_name(scope_id);
    let b = conversation_collection_name(scope_id);
    assert_eq!(a, b);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sober-memory -q -- conversation_collection`
Expected: compilation error — function doesn't exist.

- [ ] **Step 3: Implement `conversation_collection_name` and prefix constant**

Add to `collections.rs` (after `system_collection_name`):

```rust
/// Prefix for conversation-scoped Qdrant collections.
pub const CONVERSATION_COLLECTION_PREFIX: &str = "conv_";

/// Returns the collection name for a conversation's scoped memory.
///
/// Uses the simple (unhyphenated) UUID format from the scope ID.
#[must_use]
pub fn conversation_collection_name(scope_id: sober_core::ScopeId) -> String {
    format!("{}{}", CONVERSATION_COLLECTION_PREFIX, scope_id.as_uuid().simple())
}
```

- [ ] **Step 4: Export from `mod.rs`**

In `backend/crates/sober-memory/src/store/mod.rs`, update the `pub use collections` line:

```rust
pub use collections::{
    conversation_collection_name, system_collection_name, user_collection_name,
    CONVERSATION_COLLECTION_PREFIX,
};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p sober-memory -q -- conversation_collection`
Expected: all 3 PASS.

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-memory/src/store/collections.rs backend/crates/sober-memory/src/store/mod.rs
git commit -m "feat(memory): add conversation_collection_name helper"
```

---

### Task 3: New types — `StoreOutcome`, `CollectionTarget`, `DedupStats`

**Files:**
- Modify: `backend/crates/sober-memory/src/store/types.rs`
- Modify: `backend/crates/sober-memory/src/store/mod.rs`
- Modify: `backend/crates/sober-memory/src/lib.rs`

- [ ] **Step 1: Add types to `types.rs`**

Append to `backend/crates/sober-memory/src/store/types.rs`:

```rust
/// Outcome of a dedup-aware store operation.
#[derive(Debug)]
pub enum StoreOutcome {
    /// A new point was stored (no duplicate found).
    Stored { point_id: uuid::Uuid },
    /// A duplicate was found; the existing point was boosted instead.
    Deduplicated {
        existing_point_id: uuid::Uuid,
        similarity: f32,
    },
}

/// Target collection for batch operations.
#[derive(Debug, Clone)]
pub enum CollectionTarget {
    /// A specific user's collection.
    User(sober_core::UserId),
    /// A specific conversation's collection.
    Conversation(sober_core::ConversationId),
    /// The system-wide collection.
    System,
}

/// Summary statistics from a batch deduplication run.
#[derive(Debug, Default)]
pub struct DedupStats {
    /// Number of points scanned.
    pub scanned: u64,
    /// Number of duplicate points deleted.
    pub merged: u64,
}
```

- [ ] **Step 2: Export new types from `mod.rs` and `lib.rs`**

In `backend/crates/sober-memory/src/store/mod.rs`:

```rust
pub use types::{ChunkType, CollectionTarget, DedupStats, MemoryHit, StoreChunk, StoreOutcome, StoreQuery};
```

In `backend/crates/sober-memory/src/lib.rs`:

```rust
pub use store::{
    ChunkType, CollectionTarget, DedupStats, MemoryHit, MemoryStore, StoreChunk, StoreOutcome,
    StoreQuery,
};
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p sober-memory -q`
Expected: success.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-memory/src/store/types.rs backend/crates/sober-memory/src/store/mod.rs backend/crates/sober-memory/src/lib.rs
git commit -m "feat(memory): add StoreOutcome, CollectionTarget, DedupStats types"
```

---

### Task 4: Update `collection_for_scope` — route conversation scopes to `conv_` collections

**Files:**
- Modify: `backend/crates/sober-memory/src/store/memory_store.rs:22,95-99,377-383`

- [ ] **Step 1: Write unit test for routing**

Add a `#[cfg(test)]` module at the bottom of `memory_store.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sober_core::{ConversationId, ScopeId, UserId};

    fn test_store() -> MemoryStore {
        // Doesn't connect — just needs the struct for non-async methods
        MemoryStore {
            client: Arc::new(
                Qdrant::from_url("http://localhost:6334")
                    .build()
                    .unwrap(),
            ),
            dense_vector_size: 128,
        }
    }

    #[test]
    fn collection_for_scope_routes_global_to_system() {
        let store = test_store();
        let user_id = UserId::new();
        let name = store.collection_for_scope(user_id, ScopeId::GLOBAL);
        assert_eq!(name, "system");
    }

    #[test]
    fn collection_for_scope_routes_user_to_user_collection() {
        let store = test_store();
        let user_id = UserId::new();
        let user_scope = ScopeId::from_uuid(*user_id.as_uuid());
        let name = store.collection_for_scope(user_id, user_scope);
        assert!(name.starts_with("user_"), "expected user_ prefix, got {name}");
    }

    #[test]
    fn collection_for_scope_routes_conversation_to_conv_collection() {
        let store = test_store();
        let user_id = UserId::new();
        let conv_id = ConversationId::new();
        let conv_scope = ScopeId::from_uuid(*conv_id.as_uuid());
        let name = store.collection_for_scope(user_id, conv_scope);
        assert!(name.starts_with("conv_"), "expected conv_ prefix, got {name}");
    }
}
```

- [ ] **Step 2: Run tests to verify the conversation test fails**

Run: `cargo test -p sober-memory -q -- collection_for_scope`
Expected: `collection_for_scope_routes_conversation_to_conv_collection` FAILS (currently returns `user_` prefix).

- [ ] **Step 3: Update `collection_for_scope` to three-way branch**

In `memory_store.rs`, add the import at the top (with the other `collections::` imports on line 22):

```rust
use super::collections::{conversation_collection_name, system_collection_name, user_collection_name};
```

Replace the `collection_for_scope` method (lines 377-383):

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

Also update the scope label in `store()` (lines 95-99) to detect conversation scope:

```rust
let scope_label = if chunk.scope_id == ScopeId::GLOBAL {
    "global"
} else if chunk.scope_id == ScopeId::from_uuid(*user_id.as_uuid()) {
    "user"
} else {
    "conversation"
};
```

Apply the same three-way scope label in `search()` (lines 151-155):

```rust
let scope_label = if query.scope_id == ScopeId::GLOBAL {
    "global"
} else if query.scope_id == ScopeId::from_uuid(*user_id.as_uuid()) {
    "user"
} else {
    "conversation"
};
```

- [ ] **Step 4: Run tests to verify all pass**

Run: `cargo test -p sober-memory -q -- collection_for_scope`
Expected: all 3 PASS.

- [ ] **Step 5: Run full crate tests and clippy**

Run: `cargo test -p sober-memory -q && cargo clippy -p sober-memory -q -- -D warnings`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-memory/src/store/memory_store.rs
git commit -m "feat(memory): route conversation scopes to conv_ collections"
```

---

### Task 5: Add `decay_at` to `MemoryHit` and update `scored_point_to_hit`

**Files:**
- Modify: `backend/crates/sober-memory/src/store/types.rs:94-112`
- Modify: `backend/crates/sober-memory/src/store/memory_store.rs:465-510`

- [ ] **Step 1: Add `decay_at` field to `MemoryHit`**

In `types.rs`, add after the `created_at` field in `MemoryHit`:

```rust
/// When this memory starts decaying.
pub decay_at: DateTime<Utc>,
```

- [ ] **Step 2: Update `scored_point_to_hit` to extract `decay_at`**

In `memory_store.rs`, in the `scored_point_to_hit` method, add after the `created_at` extraction (after line 498):

```rust
let decay_at = Self::payload_str(payload, fields::DECAY_AT)
    .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
    .map(|dt| dt.with_timezone(&Utc))
    .unwrap_or(created_at);
```

And add `decay_at` to the `MemoryHit` construction (inside the `Some(MemoryHit { ... })`):

```rust
decay_at,
```

- [ ] **Step 3: Fix any compilation errors from the new field**

Run: `cargo build -p sober-memory -q 2>&1 | head -30`

The integration tests in `store_integration.rs` shouldn't need changes since they use `MemoryHit` from search results (not constructing it). If compilation fails elsewhere, check for any manual `MemoryHit` construction.

- [ ] **Step 4: Run tests and clippy**

Run: `cargo test -p sober-memory -q && cargo clippy -p sober-memory -q -- -D warnings`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-memory/src/store/types.rs backend/crates/sober-memory/src/store/memory_store.rs
git commit -m "feat(memory): add decay_at to MemoryHit for read-time decay"
```

---

### Task 6: `find_similar` and `store_with_dedup` methods

**Files:**
- Modify: `backend/crates/sober-memory/src/store/memory_store.rs`

- [ ] **Step 1: Add `find_similar` method**

Add to the `impl MemoryStore` block, after `apply_retrieval_boost` and before the `// -- Private helpers --` section:

```rust
/// Finds the most similar existing memory within a scope.
///
/// Dense-only cosine query with `limit(1)` and a score threshold.
/// Returns `None` if no point exceeds the threshold or the collection
/// is empty.
pub async fn find_similar(
    &self,
    user_id: UserId,
    scope_id: ScopeId,
    dense_vector: &[f32],
    threshold: f32,
) -> Result<Option<MemoryHit>, MemoryError> {
    let start = Instant::now();

    let collection = self.collection_for_scope(user_id, scope_id);

    // Check if collection exists — return None for empty/missing collections.
    if !self
        .client
        .collection_exists(&collection)
        .await
        .map_err(|e| MemoryError::Qdrant(e.to_string()))?
    {
        return Ok(None);
    }

    let scope_filter = Filter::must(vec![Condition::matches(
        fields::SCOPE_ID,
        scope_id.to_string(),
    )]);

    let qb = QueryPointsBuilder::new(&collection)
        .query(VectorInput::new_dense(dense_vector.to_vec()))
        .using(DENSE_VECTOR_NAME)
        .filter(scope_filter)
        .limit(1)
        .score_threshold(threshold)
        .with_payload(true);

    let result = self
        .client
        .query(qb)
        .await
        .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

    let hit = result
        .result
        .first()
        .and_then(|p| self.scored_point_to_hit(p));

    let elapsed = start.elapsed().as_secs_f64();
    histogram!("sober_memory_dedup_check_duration_seconds").record(elapsed);

    Ok(hit)
}

/// Stores a memory chunk with write-time deduplication.
///
/// If a sufficiently similar memory exists in the same scope, the
/// existing point's importance is boosted and no new point is stored.
/// Otherwise a new point is created via [`store`](Self::store).
pub async fn store_with_dedup(
    &self,
    user_id: UserId,
    chunk: StoreChunk,
    config: &MemoryConfig,
) -> Result<super::types::StoreOutcome, MemoryError> {
    use super::types::StoreOutcome;

    let threshold = config.dedup_similarity_threshold as f32;

    // Threshold >= 1.0 disables dedup (cosine similarity maxes at 1.0).
    if threshold >= 1.0 {
        let point_id = self.store(user_id, chunk).await?;
        return Ok(StoreOutcome::Stored { point_id });
    }

    if let Some(existing) =
        self.find_similar(user_id, chunk.scope_id, &chunk.dense_vector, threshold)
            .await?
    {
        let similarity = existing.score;
        let existing_point_id = existing.point_id;

        // Boost existing memory instead of creating a duplicate.
        self.apply_retrieval_boost(user_id, chunk.scope_id, existing_point_id, config)
            .await?;

        counter!(
            "sober_memory_dedup_total",
            "outcome" => "deduplicated",
            "chunk_type" => chunk.chunk_type.to_string(),
        )
        .increment(1);

        Ok(StoreOutcome::Deduplicated {
            existing_point_id,
            similarity,
        })
    } else {
        let chunk_type_str = chunk.chunk_type.to_string();
        let point_id = self.store(user_id, chunk).await?;

        counter!(
            "sober_memory_dedup_total",
            "outcome" => "stored",
            "chunk_type" => chunk_type_str,
        )
        .increment(1);

        Ok(StoreOutcome::Stored { point_id })
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p sober-memory -q`
Expected: success.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -p sober-memory -q -- -D warnings`
Expected: no warnings.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-memory/src/store/memory_store.rs
git commit -m "feat(memory): add find_similar and store_with_dedup methods"
```

---

### Task 7: `delete_collection`, `list_collections`, and `deduplicate` methods

**Files:**
- Modify: `backend/crates/sober-memory/src/store/memory_store.rs`

- [ ] **Step 1: Add `delete_collection` and `list_collections` methods**

Add to `impl MemoryStore`, after `store_with_dedup`:

```rust
/// Deletes an entire Qdrant collection.
///
/// Used by orphan cleanup to remove collections for deleted conversations.
/// No-op if the collection does not exist.
pub async fn delete_collection(&self, name: &str) -> Result<(), MemoryError> {
    if !self
        .client
        .collection_exists(name)
        .await
        .map_err(|e| MemoryError::Qdrant(e.to_string()))?
    {
        return Ok(());
    }

    self.client
        .delete_collection(name)
        .await
        .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

    tracing::info!(collection = name, "deleted qdrant collection");
    Ok(())
}

/// Lists all Qdrant collection names.
pub async fn list_collections(&self) -> Result<Vec<String>, MemoryError> {
    let response = self
        .client
        .list_collections()
        .await
        .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

    Ok(response
        .collections
        .into_iter()
        .map(|c| c.name)
        .collect())
}
```

- [ ] **Step 2: Add `cosine_similarity_from_points` private helper**

Add in the private helpers section (after `payload_u64`):

```rust
/// Computes cosine similarity between the dense vectors of two retrieved points.
fn cosine_similarity_from_points(
    a: &qdrant_client::qdrant::RetrievedPoint,
    b: &qdrant_client::qdrant::RetrievedPoint,
) -> f32 {
    let extract_dense = |p: &qdrant_client::qdrant::RetrievedPoint| -> Option<Vec<f32>> {
        p.vectors
            .as_ref()
            .and_then(|vs| {
                use qdrant_client::qdrant::vectors::VectorsOptions;
                match &vs.vectors_options {
                    Some(VectorsOptions::Vectors(named)) => {
                        named.vectors.get(DENSE_VECTOR_NAME).map(|v| v.data.clone())
                    }
                    _ => None,
                }
            })
    };

    let (Some(va), Some(vb)) = (extract_dense(a), extract_dense(b)) else {
        return 0.0;
    };

    if va.len() != vb.len() || va.is_empty() {
        return 0.0;
    }

    let dot: f32 = va.iter().zip(vb.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = va.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = vb.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}
```

- [ ] **Step 3: Add `deduplicate` method**

Add after `list_collections`:

```rust
/// Batch deduplication within a target collection.
///
/// Scrolls all points, computes pairwise cosine similarity within each
/// scope group, and deletes lower-importance duplicates. The survivor
/// gets an importance boost.
///
/// Returns statistics about the run.
pub async fn deduplicate(
    &self,
    target: super::types::CollectionTarget,
    config: &MemoryConfig,
) -> Result<super::types::DedupStats, MemoryError> {
    use super::types::DedupStats;

    let start = Instant::now();
    let threshold = config.dedup_similarity_threshold as f32;

    let collection = match &target {
        super::types::CollectionTarget::User(uid) => user_collection_name(*uid),
        super::types::CollectionTarget::Conversation(cid) => {
            super::collections::conversation_collection_name(
                ScopeId::from_uuid(*cid.as_uuid()),
            )
        }
        super::types::CollectionTarget::System => system_collection_name().to_owned(),
    };

    if !self
        .client
        .collection_exists(&collection)
        .await
        .map_err(|e| MemoryError::Qdrant(e.to_string()))?
    {
        return Ok(DedupStats::default());
    }

    let mut stats = DedupStats::default();
    let mut offset: Option<qdrant_client::qdrant::PointId> = None;
    let mut to_delete: Vec<qdrant_client::qdrant::PointId> = Vec::new();

    loop {
        let mut sb = ScrollPointsBuilder::new(&collection)
            .limit(100)
            .with_payload(true)
            .with_vectors(true);

        if let Some(ref o) = offset {
            sb = sb.offset(o.clone());
        }

        let result = self
            .client
            .scroll(sb)
            .await
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        let points = &result.result;
        stats.scanned += points.len() as u64;

        // Group by scope_id for pairwise comparison
        let mut scope_groups: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();

        for (i, point) in points.iter().enumerate() {
            let scope = Self::payload_str(&point.payload, fields::SCOPE_ID)
                .unwrap_or_default();
            scope_groups.entry(scope).or_default().push(i);
        }

        // Track which indices in this batch are marked for deletion
        let mut deleted_indices: std::collections::HashSet<usize> =
            std::collections::HashSet::new();

        for indices in scope_groups.values() {
            for (a_pos, &i) in indices.iter().enumerate() {
                if deleted_indices.contains(&i) {
                    continue;
                }
                for &j in &indices[a_pos + 1..] {
                    if deleted_indices.contains(&j) {
                        continue;
                    }

                    let sim = Self::cosine_similarity_from_points(&points[i], &points[j]);
                    if sim >= threshold {
                        // Keep higher importance; tiebreak by older created_at
                        let imp_i = Self::payload_f64(&points[i].payload, fields::IMPORTANCE)
                            .unwrap_or(0.0);
                        let imp_j = Self::payload_f64(&points[j].payload, fields::IMPORTANCE)
                            .unwrap_or(0.0);

                        let victim_idx = if imp_i > imp_j {
                            j
                        } else if imp_j > imp_i {
                            i
                        } else {
                            // Equal importance — keep older (lower created_at)
                            let ca_i = Self::payload_str(&points[i].payload, fields::CREATED_AT)
                                .unwrap_or_default();
                            let ca_j = Self::payload_str(&points[j].payload, fields::CREATED_AT)
                                .unwrap_or_default();
                            if ca_i <= ca_j { j } else { i }
                        };

                        if let Some(ref id) = points[victim_idx].id {
                            to_delete.push(id.clone());
                        }
                        deleted_indices.insert(victim_idx);
                    }
                }
            }
        }

        // Flush deletes in batches
        if to_delete.len() >= 50 {
            stats.merged += to_delete.len() as u64;
            self.client
                .delete_points(
                    DeletePointsBuilder::new(&collection)
                        .points(PointsIdsList {
                            ids: to_delete.drain(..).collect(),
                        })
                        .wait(true),
                )
                .await
                .map_err(|e| MemoryError::Qdrant(e.to_string()))?;
        }

        match result.next_page_offset {
            Some(next) => offset = Some(next),
            None => break,
        }
    }

    // Flush remaining deletes
    if !to_delete.is_empty() {
        stats.merged += to_delete.len() as u64;
        self.client
            .delete_points(
                DeletePointsBuilder::new(&collection)
                    .points(PointsIdsList { ids: to_delete })
                    .wait(true),
            )
            .await
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?;
    }

    let elapsed = start.elapsed().as_secs_f64();
    counter!("sober_memory_batch_dedup_runs_total").increment(1);
    histogram!("sober_memory_batch_dedup_duration_seconds").record(elapsed);
    counter!("sober_memory_batch_dedup_merged_total").increment(stats.merged);

    Ok(stats)
}
```

- [ ] **Step 4: Verify it compiles and passes clippy**

Run: `cargo build -p sober-memory -q && cargo clippy -p sober-memory -q -- -D warnings`
Expected: success.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-memory/src/store/memory_store.rs
git commit -m "feat(memory): add delete_collection, list_collections, deduplicate methods"
```

---

### Task 8: Update context loader — ensure + search conversation collections

**Files:**
- Modify: `backend/crates/sober-memory/src/loader/context_loader.rs`

- [ ] **Step 1: Add decay sorting to context loader**

Add import at the top of `context_loader.rs`:

```rust
use chrono::Utc;
use crate::scoring;
```

After the three concurrent searches complete (after line 101 `let all_user_memories = user_search_result?;`), replace the two `let` bindings with mutable versions that get sorted by decayed importance:

```rust
let mut all_conv_memories = conv_search_result?;
let mut all_user_memories = user_search_result?;

// Sort by decayed importance (highest first) for token budget packing.
let now = Utc::now();
let half_life = config.decay_half_life_days;
let sort_by_decayed = |hits: &mut Vec<MemoryHit>| {
    hits.sort_by(|a, b| {
        let elapsed_a = (now - a.decay_at).num_seconds().max(0) as f64 / 86400.0;
        let elapsed_b = (now - b.decay_at).num_seconds().max(0) as f64 / 86400.0;
        let da = scoring::decay(a.importance, elapsed_a, half_life);
        let db = scoring::decay(b.importance, elapsed_b, half_life);
        db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
    });
};
sort_by_decayed(&mut all_conv_memories);
sort_by_decayed(&mut all_user_memories);
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p sober-memory -q`
Expected: success.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -p sober-memory -q -- -D warnings`
Expected: no warnings.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-memory/src/loader/context_loader.rs
git commit -m "feat(memory): sort context memories by decayed importance"
```

---

### Task 9: Update ingestion — switch to `store_with_dedup`

**Files:**
- Modify: `backend/crates/sober-agent/src/ingestion.rs`
- Modify: `backend/crates/sober-agent/src/turn.rs` (the call site)

- [ ] **Step 1: Update `spawn_extraction_ingestion` signature**

In `ingestion.rs`, change the function signature to accept `&MemoryConfig` instead of `half_life_days: u32`:

```rust
pub fn spawn_extraction_ingestion(
    llm: &Arc<dyn LlmEngine>,
    memory: &Arc<MemoryStore>,
    user_id: UserId,
    conversation_id: ConversationId,
    extractions: Vec<MemoryExtraction>,
    memory_config: &MemoryConfig,
)
```

Add the import:

```rust
use sober_core::config::MemoryConfig;
```

- [ ] **Step 2: Update the function body to use `store_with_dedup`**

Clone the config for the spawned task:

```rust
let memory_config = memory_config.clone();
```

Update the `decay_at` calculation to use the cloned config:

```rust
let decay_at = chrono::Utc::now()
    + chrono::Duration::days(memory_config.decay_half_life_days as i64);
```

Replace `memory.store(user_id, ...)` call (lines 76-92) with:

```rust
match memory
    .store_with_dedup(
        user_id,
        StoreChunk {
            dense_vector,
            content: extraction.content,
            chunk_type,
            scope_id,
            source_message_id: None,
            importance,
            decay_at,
        },
        &memory_config,
    )
    .await
{
    Ok(outcome) => {
        debug!(?outcome, "extraction ingestion: stored memory");
    }
    Err(e) => {
        warn!(error = %e, "extraction ingestion: failed to store");
    }
}
```

- [ ] **Step 3: Update call site in `turn.rs`**

Find the `spawn_extraction_ingestion` call in `turn.rs` and update it to pass `&params.ctx.memory_config` instead of `params.ctx.memory_config.decay_half_life_days`:

```rust
crate::ingestion::spawn_extraction_ingestion(
    &params.ctx.llm,
    &params.ctx.memory,
    params.user_id,
    params.conversation_id,
    extraction_result.extractions,
    &params.ctx.memory_config,
);
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p sober-agent -q`
Expected: success.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -p sober-agent -q -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-agent/src/ingestion.rs backend/crates/sober-agent/src/turn.rs
git commit -m "feat(agent): use store_with_dedup in memory ingestion"
```

---

### Task 10: Update RememberTool — switch to `store_with_dedup`

**Files:**
- Modify: `backend/crates/sober-agent/src/tools/memory.rs`

- [ ] **Step 1: Replace `store` with `store_with_dedup` in remember handler**

In `tools/memory.rs`, find the block that calls `self.memory.store(user_id, chunk)` (around line 432-436) and replace with:

```rust
let outcome = self
    .memory
    .store_with_dedup(user_id, chunk, &self.memory_config)
    .await
    .map_err(|e| ToolError::ExecutionFailed(format!("memory store failed: {e}")))?;

let response = match outcome {
    sober_memory::StoreOutcome::Stored { point_id } => {
        format!(
            "Stored as {} (importance: {:.1}, id: {}): \"{}\"",
            chunk_type_label(chunk_type),
            importance,
            point_id,
            if content.len() > 100 {
                format!("{}...", &content[..100])
            } else {
                content
            }
        )
    }
    sober_memory::StoreOutcome::Deduplicated {
        existing_point_id,
        similarity,
    } => {
        format!(
            "Similar memory already exists (id: {}, similarity: {:.2}). Boosted existing instead of creating duplicate.",
            existing_point_id, similarity
        )
    }
};

Ok(ToolOutput {
    content: response,
    is_error: false,
})
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p sober-agent -q`
Expected: success.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -p sober-agent -q -- -D warnings`
Expected: no warnings.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-agent/src/tools/memory.rs
git commit -m "feat(agent): use store_with_dedup in remember tool"
```

---

### Task 11: Batch dedup scheduler executor

**Files:**
- Create: `backend/crates/sober-scheduler/src/executors/memory_dedup.rs`
- Modify: `backend/crates/sober-scheduler/src/executors/mod.rs`
- Modify: `backend/crates/sober-scheduler/src/main.rs`

- [ ] **Step 1: Create the executor**

Create `backend/crates/sober-scheduler/src/executors/memory_dedup.rs`:

```rust
//! Memory deduplication executor — batch-sweeps collections for duplicate
//! memories that slipped through write-time dedup.

use std::sync::Arc;

use sober_core::config::MemoryConfig;
use sober_core::error::AppError;
use sober_core::types::{Job, UserId};
use sober_memory::{CollectionTarget, MemoryStore};
use tracing::{info, instrument};

use crate::executor::{ExecutionResult, JobExecutor};

/// Operation key for job registration.
pub const OP: &str = "memory_dedup";

/// Sweeps a user's (and optionally conversation) collections for duplicates.
pub struct MemoryDedupExecutor {
    memory_store: Arc<MemoryStore>,
    memory_config: MemoryConfig,
}

impl MemoryDedupExecutor {
    /// Create a new memory dedup executor.
    pub fn new(memory_store: Arc<MemoryStore>, memory_config: MemoryConfig) -> Self {
        Self {
            memory_store,
            memory_config,
        }
    }
}

#[tonic::async_trait]
impl JobExecutor for MemoryDedupExecutor {
    #[instrument(skip(self, job), fields(job.id = %job.id, job.name = %job.name))]
    async fn execute(&self, job: &Job) -> Result<ExecutionResult, AppError> {
        let user_id = job
            .owner_id
            .map(UserId::from_uuid)
            .ok_or_else(|| AppError::Validation("memory_dedup requires owner_id".into()))?;

        // Dedup user collection
        let user_stats = self
            .memory_store
            .deduplicate(CollectionTarget::User(user_id), &self.memory_config)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        // Dedup conversation collections if specified in payload
        let mut conv_merged: u64 = 0;
        if let Some(ref payload) = job.payload {
            if let Some(conv_ids) = payload.get("conversation_ids").and_then(|v| v.as_array()) {
                for val in conv_ids {
                    if let Some(id_str) = val.as_str() {
                        if let Ok(uuid) = uuid::Uuid::parse_str(id_str) {
                            let conv_id = sober_core::ConversationId::from_uuid(uuid);
                            let stats = self
                                .memory_store
                                .deduplicate(
                                    CollectionTarget::Conversation(conv_id),
                                    &self.memory_config,
                                )
                                .await
                                .map_err(|e| AppError::Internal(e.into()))?;
                            conv_merged += stats.merged;
                        }
                    }
                }
            }
        }

        let total_merged = user_stats.merged + conv_merged;

        info!(
            user_id = %user_id,
            user_scanned = user_stats.scanned,
            user_merged = user_stats.merged,
            conv_merged,
            "memory dedup complete"
        );

        Ok(ExecutionResult {
            summary: format!(
                "deduped user {user_id}: scanned {}, merged {} (user: {}, conv: {conv_merged})",
                user_stats.scanned, total_merged, user_stats.merged
            ),
            artifact_ref: None,
        })
    }
}
```

- [ ] **Step 2: Register module in `mod.rs`**

Add to `backend/crates/sober-scheduler/src/executors/mod.rs`:

```rust
pub mod memory_dedup;
```

- [ ] **Step 3: Register executor in `main.rs`**

In `build_executor_registry`, after the `memory_pruning` registration (after line 161), add:

```rust
// Memory dedup executor — reuses the same memory store Arc
registry.register(
    sober_scheduler::executors::memory_dedup::OP,
    Arc::new(sober_scheduler::executors::memory_dedup::MemoryDedupExecutor::new(
        Arc::clone(&memory_store),
        config.memory.clone(),
    )),
);
```

Also update the pruning registration to clone the Arc instead of consuming it. Change line 158 from `memory_store,` to `Arc::clone(&memory_store),`:

```rust
registry.register(
    "memory_pruning",
    Arc::new(MemoryPruningExecutor::new(
        Arc::clone(&memory_store),
        config.memory.clone(),
    )),
);
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p sober-scheduler -q`
Expected: success.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -p sober-scheduler -q -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-scheduler/src/executors/memory_dedup.rs backend/crates/sober-scheduler/src/executors/mod.rs backend/crates/sober-scheduler/src/main.rs
git commit -m "feat(scheduler): add memory dedup batch executor"
```

---

### Task 12: Orphan collection cleanup executor

**Files:**
- Modify: `backend/crates/sober-core/src/types/repo.rs` (add `filter_existing_ids` to trait)
- Modify: `backend/crates/sober-db/src/repos/` (implement in `PgConversationRepo`)
- Create: `backend/crates/sober-scheduler/src/executors/memory_orphan_cleanup.rs`
- Modify: `backend/crates/sober-scheduler/src/executors/mod.rs`
- Modify: `backend/crates/sober-scheduler/src/main.rs`

- [ ] **Step 1: Add `filter_existing_ids` to `ConversationRepo` trait**

In `backend/crates/sober-core/src/types/repo.rs`, add to the `ConversationRepo` trait:

```rust
/// Checks which conversation IDs from the input list exist in the database.
/// Returns only the IDs that exist.
fn filter_existing_ids(
    &self,
    ids: &[ConversationId],
) -> impl Future<Output = Result<Vec<ConversationId>, AppError>> + Send;
```

- [ ] **Step 2: Implement in `PgConversationRepo`**

Find the `PgConversationRepo` implementation in `sober-db` and add:

```rust
fn filter_existing_ids(
    &self,
    ids: &[ConversationId],
) -> impl Future<Output = Result<Vec<ConversationId>, AppError>> + Send {
    let pool = self.pool.clone();
    let uuids: Vec<uuid::Uuid> = ids.iter().map(|id| *id.as_uuid()).collect();
    async move {
        let rows = sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT id FROM conversations WHERE id = ANY($1)"
        )
        .bind(&uuids)
        .fetch_all(&pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(ConversationId::from_uuid).collect())
    }
}
```

- [ ] **Step 3: Create the orphan cleanup executor**

Create `backend/crates/sober-scheduler/src/executors/memory_orphan_cleanup.rs`:

```rust
//! Orphan collection cleanup executor — deletes `conv_` Qdrant collections
//! whose conversations no longer exist in Postgres.

use std::sync::Arc;

use sober_core::error::AppError;
use sober_core::types::Job;
use sober_core::ConversationId;
use sober_memory::store::CONVERSATION_COLLECTION_PREFIX;
use sober_memory::MemoryStore;
use tracing::{info, instrument, warn};

use crate::executor::{ExecutionResult, JobExecutor};

/// Operation key for job registration.
pub const OP: &str = "memory_orphan_cleanup";

/// Deletes conversation collections whose conversations no longer exist.
pub struct MemoryOrphanCleanupExecutor<C: sober_core::ConversationRepo> {
    memory_store: Arc<MemoryStore>,
    conversation_repo: Arc<C>,
}

impl<C: sober_core::ConversationRepo> MemoryOrphanCleanupExecutor<C> {
    /// Create a new orphan cleanup executor.
    pub fn new(memory_store: Arc<MemoryStore>, conversation_repo: Arc<C>) -> Self {
        Self {
            memory_store,
            conversation_repo,
        }
    }
}

#[tonic::async_trait]
impl<C: sober_core::ConversationRepo + 'static> JobExecutor for MemoryOrphanCleanupExecutor<C> {
    #[instrument(skip(self, job), fields(job.id = %job.id, job.name = %job.name))]
    async fn execute(&self, job: &Job) -> Result<ExecutionResult, AppError> {
        let _ = job; // no job-specific params needed

        let collections = self
            .memory_store
            .list_collections()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        // Filter to conv_ collections and extract UUIDs
        let conv_collections: Vec<(String, ConversationId)> = collections
            .into_iter()
            .filter_map(|name| {
                let uuid_str = name.strip_prefix(CONVERSATION_COLLECTION_PREFIX)?;
                let uuid = uuid::Uuid::parse_str(uuid_str).ok()?;
                Some((name, ConversationId::from_uuid(uuid)))
            })
            .collect();

        if conv_collections.is_empty() {
            return Ok(ExecutionResult {
                summary: "no conversation collections found".to_owned(),
                artifact_ref: None,
            });
        }

        let all_ids: Vec<ConversationId> =
            conv_collections.iter().map(|(_, id)| *id).collect();

        let existing = self
            .conversation_repo
            .filter_existing_ids(&all_ids)
            .await?;

        let existing_set: std::collections::HashSet<ConversationId> =
            existing.into_iter().collect();

        let mut deleted = 0u64;
        for (name, id) in &conv_collections {
            if !existing_set.contains(id) {
                if let Err(e) = self.memory_store.delete_collection(name).await {
                    warn!(collection = %name, error = %e, "failed to delete orphan collection");
                } else {
                    deleted += 1;
                }
            }
        }

        metrics::counter!("sober_memory_orphan_cleanup_runs_total").increment(1);
        metrics::counter!("sober_memory_orphan_cleanup_deleted_total").increment(deleted);

        info!(
            total_conv_collections = conv_collections.len(),
            deleted,
            "orphan collection cleanup complete"
        );

        Ok(ExecutionResult {
            summary: format!(
                "orphan cleanup: checked {} conv collections, deleted {deleted} orphans",
                conv_collections.len()
            ),
            artifact_ref: None,
        })
    }
}
```

- [ ] **Step 4: Register module in `mod.rs`**

Add to `backend/crates/sober-scheduler/src/executors/mod.rs`:

```rust
pub mod memory_orphan_cleanup;
```

- [ ] **Step 5: Register executor in `main.rs`**

In `build_executor_registry`, add after the memory_dedup registration:

```rust
// Orphan collection cleanup executor
let orphan_conv_repo = Arc::new(sober_db::PgConversationRepo::new(pool.clone()));
registry.register(
    sober_scheduler::executors::memory_orphan_cleanup::OP,
    Arc::new(
        sober_scheduler::executors::memory_orphan_cleanup::MemoryOrphanCleanupExecutor::new(
            Arc::clone(&memory_store),
            orphan_conv_repo,
        ),
    ),
);
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo build -p sober-scheduler -q`
Expected: success.

- [ ] **Step 7: Run clippy across affected crates**

Run: `cargo clippy -p sober-core -p sober-db -p sober-scheduler -q -- -D warnings`
Expected: no warnings.

- [ ] **Step 8: Commit**

```bash
git add backend/crates/sober-core/src/types/repo.rs backend/crates/sober-db/src/repos/ backend/crates/sober-scheduler/src/executors/memory_orphan_cleanup.rs backend/crates/sober-scheduler/src/executors/mod.rs backend/crates/sober-scheduler/src/main.rs
git commit -m "feat(scheduler): add orphan collection cleanup executor"
```

---

### Task 13: Integration tests

**Files:**
- Modify: `backend/crates/sober-memory/tests/store_integration.rs`

- [ ] **Step 1: Add test helpers**

Add at the top of `store_integration.rs` (after existing imports):

```rust
use sober_memory::StoreOutcome;
```

Update the `memory_config()` helper to include the new field:

```rust
fn memory_config() -> MemoryConfig {
    MemoryConfig {
        decay_half_life_days: 30,
        retrieval_boost: 0.2,
        prune_threshold: 0.1,
        dedup_similarity_threshold: 0.92,
    }
}
```

Add a helper for creating chunks with specific vectors:

```rust
fn test_chunk_with_vector(scope_id: ScopeId, content: &str, vector: Vec<f32>) -> StoreChunk {
    StoreChunk {
        dense_vector: vector,
        content: content.to_owned(),
        chunk_type: ChunkType::Fact,
        scope_id,
        source_message_id: Some(MessageId::new()),
        importance: 0.5,
        decay_at: Utc::now(),
    }
}
```

- [ ] **Step 2: Add `store_with_dedup_allows_unique_memories` test**

```rust
#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn store_with_dedup_allows_unique_memories() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let user_id = UserId::new();
    let scope_id = ScopeId::from_uuid(*user_id.as_uuid());
    let config = memory_config();

    let v1 = vec![1.0; 128];
    let v2 = vec![-1.0; 128];

    let c1 = test_chunk_with_vector(scope_id, "cats are great pets", v1);
    let c2 = test_chunk_with_vector(scope_id, "quantum physics is complex", v2);

    let r1 = store.store_with_dedup(user_id, c1, &config).await.unwrap();
    let r2 = store.store_with_dedup(user_id, c2, &config).await.unwrap();

    assert!(matches!(r1, StoreOutcome::Stored { .. }));
    assert!(matches!(r2, StoreOutcome::Stored { .. }));
}
```

- [ ] **Step 3: Add `store_with_dedup_detects_duplicate` test**

```rust
#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn store_with_dedup_detects_duplicate() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let user_id = UserId::new();
    let scope_id = ScopeId::from_uuid(*user_id.as_uuid());
    let config = memory_config();

    let vector = vec![0.5; 128];

    let c1 = test_chunk_with_vector(scope_id, "rust is fast", vector.clone());
    let c2 = test_chunk_with_vector(scope_id, "rust is fast and safe", vector);

    let r1 = store.store_with_dedup(user_id, c1, &config).await.unwrap();
    assert!(matches!(r1, StoreOutcome::Stored { .. }));

    let r2 = store.store_with_dedup(user_id, c2, &config).await.unwrap();
    assert!(
        matches!(r2, StoreOutcome::Deduplicated { .. }),
        "identical vectors should be deduplicated, got {r2:?}"
    );
}
```

- [ ] **Step 4: Add `store_with_dedup_respects_scope_isolation` test**

```rust
#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn store_with_dedup_respects_scope_isolation() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let user_id = UserId::new();
    let config = memory_config();

    let scope_a = ScopeId::from_uuid(*user_id.as_uuid());
    let scope_b = ScopeId::from_uuid(uuid::Uuid::new_v4()); // conversation scope

    let vector = vec![0.5; 128];

    let c1 = test_chunk_with_vector(scope_a, "same content", vector.clone());
    let c2 = test_chunk_with_vector(scope_b, "same content", vector);

    let r1 = store.store_with_dedup(user_id, c1, &config).await.unwrap();
    let r2 = store.store_with_dedup(user_id, c2, &config).await.unwrap();

    // Different scopes → both should be stored
    assert!(matches!(r1, StoreOutcome::Stored { .. }));
    assert!(matches!(r2, StoreOutcome::Stored { .. }));
}
```

- [ ] **Step 5: Add `find_similar_returns_none_for_empty_collection` test**

```rust
#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn find_similar_returns_none_for_empty_collection() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let user_id = UserId::new();
    let scope_id = ScopeId::from_uuid(*user_id.as_uuid());

    let result = store
        .find_similar(user_id, scope_id, &vec![0.5; 128], 0.9)
        .await
        .unwrap();
    assert!(result.is_none());
}
```

- [ ] **Step 6: Add `store_routes_conversation_scope_to_conv_collection` test**

```rust
#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn store_routes_conversation_scope_to_conv_collection() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let user_id = UserId::new();
    let conv_scope = ScopeId::from_uuid(uuid::Uuid::new_v4());

    let chunk = test_chunk_with_vector(conv_scope, "conversation memory", vec![0.3; 128]);
    let point_id = store.store(user_id, chunk).await.unwrap();

    // Search in conversation scope should find it
    let query = StoreQuery {
        dense_vector: vec![0.3; 128],
        query_text: "conversation".to_owned(),
        scope_id: conv_scope,
        limit: 10,
        score_threshold: None,
        chunk_type_filter: None,
    };
    let results = store.search(user_id, query).await.unwrap();
    assert!(
        results.iter().any(|h| h.point_id == point_id),
        "conversation memory should be found in conv_ collection"
    );

    // Search in user scope should NOT find it
    let user_scope = ScopeId::from_uuid(*user_id.as_uuid());
    let user_query = StoreQuery {
        dense_vector: vec![0.3; 128],
        query_text: "conversation".to_owned(),
        scope_id: user_scope,
        limit: 10,
        score_threshold: None,
        chunk_type_filter: None,
    };
    let user_results = store.search(user_id, user_query).await.unwrap();
    assert!(
        !user_results.iter().any(|h| h.point_id == point_id),
        "conversation memory should NOT appear in user collection"
    );
}
```

- [ ] **Step 7: Add `deduplicate_merges_similar_points` test**

```rust
#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn deduplicate_merges_similar_points() {
    use sober_memory::CollectionTarget;

    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let user_id = UserId::new();
    let scope_id = ScopeId::from_uuid(*user_id.as_uuid());
    let config = memory_config();

    let vector = vec![0.7; 128];

    // Store two identical-vector points directly (bypass dedup to test batch)
    let c1 = test_chunk_with_vector(scope_id, "duplicate A", vector.clone());
    let c2 = test_chunk_with_vector(scope_id, "duplicate B", vector);

    store.store(user_id, c1).await.unwrap();
    store.store(user_id, c2).await.unwrap();

    let stats = store
        .deduplicate(CollectionTarget::User(user_id), &config)
        .await
        .unwrap();

    assert!(
        stats.merged > 0,
        "batch dedup should merge identical vectors, merged: {}",
        stats.merged
    );
}
```

- [ ] **Step 8: Add `delete_collection_removes_conversation_memories` test**

```rust
#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn delete_collection_removes_conversation_memories() {
    use sober_memory::store::conversation_collection_name;

    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let user_id = UserId::new();
    let conv_scope = ScopeId::from_uuid(uuid::Uuid::new_v4());

    let chunk = test_chunk_with_vector(conv_scope, "temp memory", vec![0.4; 128]);
    store.store(user_id, chunk).await.unwrap();

    let col_name = conversation_collection_name(conv_scope);
    store.delete_collection(&col_name).await.unwrap();

    // Searching should return empty (collection gone)
    let query = StoreQuery {
        dense_vector: vec![0.4; 128],
        query_text: "temp".to_owned(),
        scope_id: conv_scope,
        limit: 10,
        score_threshold: None,
        chunk_type_filter: None,
    };
    let results = store.search(user_id, query).await.unwrap();
    assert!(results.is_empty(), "collection should be deleted");
}
```

- [ ] **Step 9: Run all integration tests**

Run: `cargo test -p sober-memory -q -- --ignored`
Expected: all tests pass (requires Qdrant running).

- [ ] **Step 10: Commit**

```bash
git add backend/crates/sober-memory/tests/store_integration.rs
git commit -m "test(memory): add dedup and collection routing integration tests"
```

---

### Task 14: Workspace build verification

**Files:** None (verification only)

- [ ] **Step 1: Run workspace build**

Run: `cargo build --workspace -q`
Expected: success.

- [ ] **Step 2: Run workspace clippy**

Run: `cargo clippy --workspace -q -- -D warnings`
Expected: no warnings.

- [ ] **Step 3: Run workspace tests (unit only)**

Run: `cargo test --workspace -q`
Expected: all pass.

- [ ] **Step 4: Run integration tests (if Docker/Qdrant available)**

Run: `cargo test -p sober-memory -q -- --ignored`
Expected: all pass.

- [ ] **Step 5: Check for sqlx prepare if needed**

If any sqlx queries were added (the `filter_existing_ids` query in sober-db), run:

```bash
cd backend && cargo sqlx prepare --workspace
```

Commit the `.sqlx/` changes if updated.

- [ ] **Step 6: Move plan to active**

```bash
git mv docs/plans/pending/052-memory-deduplication docs/plans/active/052-memory-deduplication
```

- [ ] **Step 7: Final commit**

```bash
git add -A
git commit -m "chore: move #052 to active, sqlx prepare"
```
