# #034 Plan: Blob Store Garbage Collection

**Goal:** Prevent unbounded blob accumulation by collecting orphaned blobs that
are no longer referenced by any plugin config or artifact record.

**Problem:** `BlobStore` is content-addressed and write-only. Blobs are created
when plugins are generated but never deleted when plugins are removed,
regenerated (old blob superseded), or artifacts are archived/deleted. Over time,
orphaned blobs accumulate on disk with no cleanup mechanism.

**Orphan sources:**
1. Plugin deleted → config with `blob_key` removed, blob remains
2. Plugin regenerated → old `blob_key` replaced, old blob remains
3. Audit failure → blob stored before install, install rejected
4. Artifact archived/deleted → blob still on disk

---

## Design

### Approach: Mark-and-sweep GC

A periodic job (scheduler `Internal` type) scans all blobs, checks whether
each is referenced, and deletes unreferenced ones older than a grace period.

**Why not reference counting:** Ref counting requires transactional updates
across blob store + plugin table + artifact table. Mark-and-sweep is simpler,
idempotent, and tolerant of partial failures.

### Reference sources

A blob is "referenced" if ANY of these are true:
- A plugin row has `config->>'blob_key' = key`
- An artifact row has `blob_ref = key` AND state NOT IN (`Deleted`)
- The blob was created less than `GRACE_PERIOD` ago (default: 1 hour)

The grace period prevents deleting blobs that are mid-installation (stored
but not yet linked to a plugin/artifact record).

### Execution model

- Runs as a scheduler `Internal` job via `JobExecutorRegistry`
- Default schedule: daily at 03:00 UTC (configurable via `BLOB_GC_CRON`)
- Also triggerable via `sober gc blobs` (admin CLI)
- Logs each deletion at `info!` level with blob key + size

---

## Tasks

### Task 1: Add blob listing to BlobStore

**File:** `sober-workspace/src/blob.rs`

- [ ] Add `list_keys() -> Result<Vec<(String, SystemTime)>, WorkspaceError>`
  that walks the blob directory tree and returns `(key, modified_time)` pairs
- [ ] Add `total_size() -> Result<u64, WorkspaceError>` for metrics

### Task 2: Add reference-checking queries

**File:** `sober-db/src/repos/plugin.rs`, `sober-db/src/repos/artifacts.rs`

- [ ] `PgPluginRepo::blob_keys_in_use() -> Result<HashSet<String>, AppError>`
  — `SELECT DISTINCT config->>'blob_key' FROM plugins WHERE config ? 'blob_key'`
- [ ] `PgArtifactRepo::blob_refs_in_use() -> Result<HashSet<String>, AppError>`
  — `SELECT DISTINCT blob_ref FROM artifacts WHERE state != 'deleted' AND blob_ref IS NOT NULL`

### Task 3: Implement GC logic

**File:** `sober-workspace/src/blob_gc.rs` (new)

- [ ] `BlobGc::new(blob_store, plugin_repo, artifact_repo, grace_period)`
- [ ] `run() -> GcReport` — mark-and-sweep:
  1. List all blob keys + timestamps
  2. Collect referenced keys from both sources
  3. Delete blobs where: not referenced AND older than grace period
  4. Return `GcReport { scanned, deleted, bytes_freed, errors }`
- [ ] Emit metrics: `sober_workspace_blob_gc_*`

### Task 4: Register as scheduler job executor

**File:** `sober-scheduler/src/executors/blob_gc.rs` (new)

- [ ] Implement `JobExecutor` trait for `BlobGcExecutor`
- [ ] Register in `JobExecutorRegistry` at scheduler startup
- [ ] Default cron: `0 3 * * *` (daily 03:00 UTC)

### Task 5: Add CLI command

**File:** `sober-cli/src/commands/gc.rs` (new)

- [ ] `sober gc blobs` — runs GC immediately via admin socket
- [ ] `sober gc blobs --dry-run` — report what would be deleted
- [ ] Print `GcReport` summary

### Task 6: Wire plugin deletion to cache eviction

**File:** `sober-plugin/src/registry.rs`

- [ ] `PluginRegistry::uninstall()` should call `evict_wasm_host()` before
  deleting the DB record, so stale hosts don't serve deleted plugins
- [ ] This is a bug fix independent of GC but discovered alongside it

---

## Acceptance criteria

- [ ] Orphaned blobs are deleted by periodic GC
- [ ] Grace period prevents race with in-progress installations
- [ ] Referenced blobs are never deleted
- [ ] `sober gc blobs --dry-run` reports correctly
- [ ] Metrics track GC runs, deletions, bytes freed
- [ ] Plugin deletion evicts WASM host from cache
