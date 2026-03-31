# #034 Design: Blob Store Garbage Collection

## Problem

`BlobStore` is content-addressed and write-only. Blobs accumulate when:

1. A plugin is deleted — DB row removed, blob remains on disk.
2. A plugin is regenerated — old `wasm_blob_key` replaced, old blob remains.
3. An audit failure — blob stored before install, install rejected.
4. A crash mid-operation — blob stored but never linked to anything.

No cleanup mechanism exists. Over time orphaned blobs grow unboundedly.

## Approach: Mark-and-Sweep GC

A periodic scheduler job scans all blobs, checks whether each is referenced,
and deletes unreferenced ones older than a grace period.

GC is the **sole mechanism** for blob deletion. No other code path deletes
blobs — plugin uninstall, regeneration, and artifact archival all leave
cleanup to GC. This keeps blob deletion logic in one place.

**Why not reference counting:** ref counting requires transactional updates
across blob store + every referencing table. Mark-and-sweep is simpler,
idempotent, and tolerant of partial failures.

## Reference Model

A blob is "referenced" if ANY of these are true:

- A plugin row has `config->>'wasm_blob_key' = key` OR
  `config->>'manifest_blob_key' = key`.
- An artifact row has `blob_key = key` AND `state != 'archived'`
  (draft, proposed, approved, rejected all retain their blobs).
- The blob was created less than `GRACE_PERIOD` ago (default: 1 hour).

The grace period prevents deleting blobs that are mid-installation (stored
but not yet linked to any DB record).

## GC Location & Execution

The mark-and-sweep logic lives in `BlobGcExecutor` inside **sober-scheduler**.
The scheduler already depends on both `sober-workspace` (BlobStore) and
`sober-db` (repos) — no new cross-crate dependencies needed.

- Registered as a system job in `JobExecutorRegistry` alongside existing
  executors (memory_pruning, session_cleanup, plugin_cleanup,
  trait_evolution_check).
- Default cron: `0 3 * * *` (daily 03:00 UTC), configurable via
  `BLOB_GC_CRON` env var.
- CLI trigger: `sober gc blobs` uses the scheduler's existing `force_run`
  gRPC RPC.

## Components

### BlobStore Additions

Two methods added to `sober-workspace/src/blob.rs`:

- `list_keys() -> Result<Vec<(String, SystemTime)>>` — walks the blob
  directory tree, returns `(key, modified_time)` pairs.
- `total_size() -> Result<u64>` — sums all blob file sizes for
  metrics/reporting.

### Repo Query Methods

- `PgPluginRepo::blob_keys_in_use() -> Result<HashSet<String>>` — queries
  `config->>'wasm_blob_key'` and `config->>'manifest_blob_key'` from all
  plugin rows.
- `PgArtifactRepo::blob_keys_in_use() -> Result<HashSet<String>>` — queries
  `blob_key` from artifacts where `state != 'archived'` and `blob_key IS
  NOT NULL`.

### Metrics & Logging

Each deletion logged at `info!` (blob key + size). Emits counters:
`sober_blob_gc_runs_total`, `sober_blob_gc_deleted_total`,
`sober_blob_gc_bytes_freed_total`. The executor returns an
`ExecutionResult` with a human-readable summary.

### CLI Command

`sober gc blobs` — connects to the scheduler's admin socket and calls
`force_run` on the `blob_gc` system job.

## Bug Fix: Plugin Cache Eviction

`PluginManager` gets a new `uninstall()` method that calls
`self.evict_wasm_host(&id)` before delegating to
`self.registry.uninstall(id)`. This ensures deleted plugins don't serve
stale WASM hosts from cache. Independent of GC but discovered alongside it.

## Out of Scope

- Blob compaction or deduplication.
- S3-backed blob store (current: filesystem only).
- UI for blob management.
- Eager blob deletion on plugin uninstall/regeneration (GC handles it).
