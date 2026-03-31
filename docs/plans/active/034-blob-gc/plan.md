# #034 Plan: Blob Store Garbage Collection

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent unbounded blob accumulation by collecting orphaned blobs
that are no longer referenced by any plugin config or active artifact record.

**Architecture:** Mark-and-sweep GC runs as a scheduler system job. It lists
all blobs on disk, collects referenced keys from plugin configs and non-archived
artifacts, and deletes unreferenced blobs older than a grace period. GC is the
sole mechanism for blob deletion — no other code path deletes blob files.
`PluginManager` gets an `uninstall()` method that evicts the WASM cache before
deleting the plugin row (bug fix, no blob deletion).

**Tech Stack:** Rust (cargo workspace), PostgreSQL (sqlx), filesystem I/O

---

### Task 1: Add `list_keys()` and `total_size()` to BlobStore

**Files:**
- Modify: `backend/crates/sober-workspace/src/blob.rs`

- [ ] **Step 1: Write failing test for `list_keys`**

```rust
#[tokio::test]
async fn list_keys_returns_stored_blobs() {
    let tmp = TempDir::new().unwrap();
    let store = BlobStore::new(tmp.path().to_path_buf());

    let key1 = store.store(b"blob one").await.unwrap();
    let key2 = store.store(b"blob two").await.unwrap();

    let keys = store.list_keys().await.unwrap();
    let key_strs: Vec<&str> = keys.iter().map(|(k, _)| k.as_str()).collect();

    assert_eq!(keys.len(), 2);
    assert!(key_strs.contains(&key1.as_str()));
    assert!(key_strs.contains(&key2.as_str()));
}

#[tokio::test]
async fn list_keys_empty_store() {
    let tmp = TempDir::new().unwrap();
    let store = BlobStore::new(tmp.path().to_path_buf());

    let keys = store.list_keys().await.unwrap();
    assert!(keys.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sober-workspace -q -- list_keys`
Expected: FAIL — `list_keys` method does not exist

- [ ] **Step 3: Implement `list_keys`**

Add to the `impl BlobStore` block in `blob.rs`:

```rust
/// Lists all blob keys and their modification times.
///
/// Walks the blob directory tree (`{root}/{prefix}/{key}`) and returns
/// `(key, modified_time)` pairs for every blob file found.
pub async fn list_keys(&self) -> Result<Vec<(String, std::time::SystemTime)>, WorkspaceError> {
    let root = self.root.clone();
    tokio::task::spawn_blocking(move || {
        let mut entries = Vec::new();
        let read_dir = match std::fs::read_dir(&root) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(entries),
            Err(e) => return Err(WorkspaceError::Filesystem(e)),
        };
        for prefix_entry in read_dir {
            let prefix_entry = prefix_entry.map_err(WorkspaceError::Filesystem)?;
            if !prefix_entry.path().is_dir() {
                continue;
            }
            let sub_dir = std::fs::read_dir(prefix_entry.path())
                .map_err(WorkspaceError::Filesystem)?;
            for blob_entry in sub_dir {
                let blob_entry = blob_entry.map_err(WorkspaceError::Filesystem)?;
                let path = blob_entry.path();
                if path.is_file() {
                    let key = path
                        .file_name()
                        .expect("blob file always has a name")
                        .to_string_lossy()
                        .into_owned();
                    let modified = blob_entry
                        .metadata()
                        .map_err(WorkspaceError::Filesystem)?
                        .modified()
                        .map_err(WorkspaceError::Filesystem)?;
                    entries.push((key, modified));
                }
            }
        }
        Ok(entries)
    })
    .await
    .expect("spawn_blocking join")
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p sober-workspace -q -- list_keys`
Expected: PASS

- [ ] **Step 5: Write failing test for `total_size`**

```rust
#[tokio::test]
async fn total_size_sums_all_blobs() {
    let tmp = TempDir::new().unwrap();
    let store = BlobStore::new(tmp.path().to_path_buf());

    let data1 = b"hello";
    let data2 = b"world!!!";
    store.store(data1).await.unwrap();
    store.store(data2).await.unwrap();

    let size = store.total_size().await.unwrap();
    assert_eq!(size, (data1.len() + data2.len()) as u64);
}

#[tokio::test]
async fn total_size_empty_store() {
    let tmp = TempDir::new().unwrap();
    let store = BlobStore::new(tmp.path().to_path_buf());

    let size = store.total_size().await.unwrap();
    assert_eq!(size, 0);
}
```

- [ ] **Step 6: Run tests to verify they fail**

Run: `cargo test -p sober-workspace -q -- total_size`
Expected: FAIL — `total_size` method does not exist

- [ ] **Step 7: Implement `total_size`**

```rust
/// Returns the total size in bytes of all stored blobs.
pub async fn total_size(&self) -> Result<u64, WorkspaceError> {
    let root = self.root.clone();
    tokio::task::spawn_blocking(move || {
        let mut total: u64 = 0;
        let read_dir = match std::fs::read_dir(&root) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(WorkspaceError::Filesystem(e)),
        };
        for prefix_entry in read_dir {
            let prefix_entry = prefix_entry.map_err(WorkspaceError::Filesystem)?;
            if !prefix_entry.path().is_dir() {
                continue;
            }
            let sub_dir = std::fs::read_dir(prefix_entry.path())
                .map_err(WorkspaceError::Filesystem)?;
            for blob_entry in sub_dir {
                let blob_entry = blob_entry.map_err(WorkspaceError::Filesystem)?;
                if blob_entry.path().is_file() {
                    total += blob_entry
                        .metadata()
                        .map_err(WorkspaceError::Filesystem)?
                        .len();
                }
            }
        }
        Ok(total)
    })
    .await
    .expect("spawn_blocking join")
}
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p sober-workspace -q -- total_size`
Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add backend/crates/sober-workspace/src/blob.rs
git commit -m "feat(workspace): add list_keys and total_size to BlobStore"
```

---

### Task 2: Add `blob_keys_in_use()` to plugin and artifact repos

**Files:**
- Modify: `backend/crates/sober-core/src/types/repo.rs`
- Modify: `backend/crates/sober-db/src/repos/plugin.rs`
- Modify: `backend/crates/sober-db/src/repos/artifacts.rs`

- [ ] **Step 1: Add `blob_keys_in_use` to `PluginRepo` trait**

In `backend/crates/sober-core/src/types/repo.rs`, add to the `PluginRepo` trait:

```rust
/// Returns all blob keys currently referenced by plugin configs.
fn blob_keys_in_use(&self) -> impl Future<Output = Result<HashSet<String>, AppError>> + Send;
```

Add `use std::collections::HashSet;` to the imports at the top of the file if
not already present.

- [ ] **Step 2: Add `blob_keys_in_use` to `ArtifactRepo` trait**

In the same file, add to the `ArtifactRepo` trait:

```rust
/// Returns all blob keys referenced by non-archived artifacts.
fn blob_keys_in_use(&self) -> impl Future<Output = Result<HashSet<String>, AppError>> + Send;
```

- [ ] **Step 3: Implement `PgPluginRepo::blob_keys_in_use`**

In `backend/crates/sober-db/src/repos/plugin.rs`, add the implementation:

```rust
async fn blob_keys_in_use(&self) -> Result<HashSet<String>, AppError> {
    let rows: Vec<(Option<String>,)> = sqlx::query_as(
        "SELECT config->>'wasm_blob_key' AS key FROM plugins \
         WHERE config->>'wasm_blob_key' IS NOT NULL \
         UNION \
         SELECT config->>'manifest_blob_key' AS key FROM plugins \
         WHERE config->>'manifest_blob_key' IS NOT NULL",
    )
    .fetch_all(&self.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(rows.into_iter().filter_map(|(k,)| k).collect())
}
```

Add `use std::collections::HashSet;` to the file imports.

- [ ] **Step 4: Implement `PgArtifactRepo::blob_keys_in_use`**

In `backend/crates/sober-db/src/repos/artifacts.rs`, add the implementation:

```rust
async fn blob_keys_in_use(&self) -> Result<HashSet<String>, AppError> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT blob_key FROM artifacts \
         WHERE blob_key IS NOT NULL AND state != 'archived'",
    )
    .fetch_all(&self.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(rows.into_iter().map(|(k,)| k).collect())
}
```

Add `use std::collections::HashSet;` to the file imports.

- [ ] **Step 5: Verify workspace compiles**

Run: `cargo build -q --workspace`
Expected: success

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-core/src/types/repo.rs \
       backend/crates/sober-db/src/repos/plugin.rs \
       backend/crates/sober-db/src/repos/artifacts.rs
git commit -m "feat(db): add blob_keys_in_use to plugin and artifact repos"
```

---

### Task 3: Implement BlobGcExecutor

**Files:**
- Create: `backend/crates/sober-scheduler/src/executors/blob_gc.rs`
- Modify: `backend/crates/sober-scheduler/src/executors/mod.rs`

- [ ] **Step 1: Create `blob_gc.rs` with the executor**

```rust
//! Blob garbage collection executor — deletes orphaned blobs from the store.
//!
//! A blob is "referenced" if a plugin config or a non-archived artifact
//! points to it. Unreferenced blobs older than a grace period are deleted.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use sober_core::error::AppError;
use sober_core::types::Job;
use sober_core::types::repo::{ArtifactRepo, PluginRepo};
use sober_workspace::BlobStore;
use tracing::{info, warn};

use crate::executor::{ExecutionResult, JobExecutor};

/// Default grace period: blobs younger than this are never deleted,
/// even if unreferenced (protects mid-installation blobs).
const DEFAULT_GRACE_PERIOD: Duration = Duration::from_secs(3600);

/// Deletes orphaned blobs not referenced by any plugin or active artifact.
pub struct BlobGcExecutor<P: PluginRepo, A: ArtifactRepo> {
    blob_store: Arc<BlobStore>,
    plugin_repo: P,
    artifact_repo: A,
    grace_period: Duration,
}

impl<P: PluginRepo, A: ArtifactRepo> BlobGcExecutor<P, A> {
    /// Create a new blob GC executor.
    pub fn new(
        blob_store: Arc<BlobStore>,
        plugin_repo: P,
        artifact_repo: A,
    ) -> Self {
        Self {
            blob_store,
            plugin_repo,
            artifact_repo,
            grace_period: DEFAULT_GRACE_PERIOD,
        }
    }
}

#[tonic::async_trait]
impl<P: PluginRepo + 'static, A: ArtifactRepo + 'static> JobExecutor
    for BlobGcExecutor<P, A>
{
    async fn execute(&self, _job: &Job) -> Result<ExecutionResult, AppError> {
        // 1. List all blobs on disk.
        let all_blobs = self.blob_store.list_keys().await.map_err(|e| {
            AppError::Internal(anyhow::anyhow!("failed to list blobs: {e}"))
        })?;
        let scanned = all_blobs.len();

        // 2. Collect referenced keys from both sources.
        let mut referenced: HashSet<String> = self.plugin_repo.blob_keys_in_use().await?;
        let artifact_keys = self.artifact_repo.blob_keys_in_use().await?;
        referenced.extend(artifact_keys);

        // 3. Find and delete unreferenced blobs older than grace period.
        let cutoff = SystemTime::now() - self.grace_period;
        let mut deleted = 0u64;
        let mut bytes_freed = 0u64;
        let mut errors = Vec::new();

        for (key, modified) in &all_blobs {
            if referenced.contains(key) {
                continue;
            }
            if *modified > cutoff {
                continue;
            }

            // Get size before deleting.
            let path = self.blob_store.blob_path(key);
            let size = tokio::fs::metadata(&path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);

            match self.blob_store.delete(key).await {
                Ok(()) => {
                    info!(blob_key = %key, size, "deleted orphaned blob");
                    deleted += 1;
                    bytes_freed += size;
                }
                Err(e) => {
                    warn!(blob_key = %key, error = %e, "failed to delete orphaned blob");
                    errors.push(format!("{key}: {e}"));
                }
            }
        }

        metrics::counter!("sober_blob_gc_runs_total").increment(1);
        metrics::counter!("sober_blob_gc_deleted_total").increment(deleted);
        metrics::counter!("sober_blob_gc_bytes_freed_total").increment(bytes_freed);

        let summary = format!(
            "blob GC: scanned {scanned}, deleted {deleted}, freed {bytes_freed} bytes, {} errors",
            errors.len()
        );
        info!("{summary}");

        Ok(ExecutionResult {
            summary,
            artifact_ref: None,
        })
    }
}
```

- [ ] **Step 2: Add module to `executors/mod.rs`**

In `backend/crates/sober-scheduler/src/executors/mod.rs`, add:

```rust
pub mod blob_gc;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -q -p sober-scheduler`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-scheduler/src/executors/blob_gc.rs \
       backend/crates/sober-scheduler/src/executors/mod.rs
git commit -m "feat(scheduler): add BlobGcExecutor"
```

---

### Task 4: Register system job and executor

**Files:**
- Modify: `backend/crates/sober-scheduler/src/system_jobs.rs`
- Modify: `backend/crates/sober-scheduler/src/main.rs`

- [ ] **Step 1: Add system job definition**

In `system_jobs.rs`, add to the `system_jobs()` vec:

```rust
SystemJobDef {
    name: "system::blob_gc",
    schedule: "0 0 3 * * * *",
    payload: serde_json::json!({
        "type": "internal",
        "op": "blob_gc",
    }),
},
```

- [ ] **Step 2: Register executor in `main.rs`**

In `build_executor_registry()`, after the existing `blob_store` is created
(around line 170) and before `registry.register("artifact", ...)`, add:

```rust
// Blob GC executor — reuses the same blob_store Arc
let gc_plugin_repo = sober_db::PgPluginRepo::new(pool.clone());
let gc_artifact_repo = sober_db::PgArtifactRepo::new(pool.clone());
registry.register(
    "blob_gc",
    Arc::new(BlobGcExecutor::new(
        Arc::clone(&blob_store),
        gc_plugin_repo,
        gc_artifact_repo,
    )),
);
```

Add the import at the top of `main.rs`:

```rust
use sober_scheduler::executors::blob_gc::BlobGcExecutor;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -q -p sober-scheduler`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-scheduler/src/system_jobs.rs \
       backend/crates/sober-scheduler/src/main.rs
git commit -m "feat(scheduler): register blob_gc system job and executor"
```

---

### Task 5: Add `sober gc blobs` CLI command

**Files:**
- Create: `backend/crates/sober-cli/src/commands/gc.rs`
- Modify: `backend/crates/sober-cli/src/commands/mod.rs`
- Modify: `backend/crates/sober-cli/src/cli.rs`
- Modify: `backend/crates/sober-cli/src/sober.rs`

- [ ] **Step 1: Add `GcCommand` to CLI definitions**

In `backend/crates/sober-cli/src/cli.rs`, add to the `Command` enum:

```rust
/// Garbage collection commands.
#[command(subcommand)]
Gc(GcCommand),
```

Add the `GcCommand` enum below the existing command enums:

```rust
/// Garbage collection subcommands.
#[derive(Debug, Subcommand)]
pub enum GcCommand {
    /// Run blob garbage collection (requires running sober-scheduler).
    Blobs {
        /// Path to scheduler socket.
        #[arg(long, default_value = DEFAULT_SCHEDULER_SOCKET)]
        socket: String,
    },
}
```

- [ ] **Step 2: Create `gc.rs` command handler**

Create `backend/crates/sober-cli/src/commands/gc.rs`:

```rust
//! Garbage collection commands.

use anyhow::{Context, Result};

use crate::cli::GcCommand;

/// Execute a GC subcommand.
pub async fn handle(cmd: GcCommand) -> Result<()> {
    match cmd {
        GcCommand::Blobs { socket } => run_blob_gc(&socket).await,
    }
}

/// Trigger blob GC via the scheduler's force_run RPC.
///
/// Looks up the `system::blob_gc` job by listing system jobs, then calls
/// `force_run` on it.
async fn run_blob_gc(socket: &str) -> Result<()> {
    use hyper_util::rt::TokioIo;
    use tonic::transport::{Endpoint, Uri};
    use tower::service_fn;

    mod proto {
        tonic::include_proto!("sober.scheduler.v1");
    }

    use proto::scheduler_service_client::SchedulerServiceClient;
    use proto::{ForceRunRequest, ListJobsRequest};

    let path = std::path::PathBuf::from(socket);
    if !path.exists() {
        anyhow::bail!(
            "scheduler socket not found at {} — is sober-scheduler running?",
            socket
        );
    }

    let channel = Endpoint::try_from("http://[::]:50051")
        .context("invalid endpoint URI")?
        .connect_with_connector(service_fn(move |_: Uri| {
            let path = path.clone();
            async move {
                let stream = tokio::net::UnixStream::connect(path).await?;
                Ok::<_, std::io::Error>(TokioIo::new(stream))
            }
        }))
        .await
        .with_context(|| format!("failed to connect to scheduler at {socket}"))?;

    let mut client = SchedulerServiceClient::new(channel);

    // Find the blob_gc system job.
    let resp = client
        .list_jobs(ListJobsRequest {
            owner_type: "system".into(),
            owner_id: None,
            statuses: vec![],
            workspace_id: String::new(),
            name_filter: "blob_gc".into(),
        })
        .await
        .context("failed to list jobs")?;

    let jobs = resp.into_inner().jobs;
    let job = jobs
        .iter()
        .find(|j| j.name == "system::blob_gc")
        .ok_or_else(|| anyhow::anyhow!("system::blob_gc job not found — is blob GC registered?"))?;

    let resp = client
        .force_run(ForceRunRequest {
            job_id: job.id.clone(),
        })
        .await
        .context("failed to trigger blob GC")?;

    if resp.into_inner().accepted {
        println!("blob GC triggered (job {})", job.id);
    } else {
        println!("blob GC rejected (job may already be running)");
    }

    Ok(())
}
```

- [ ] **Step 3: Register module in `commands/mod.rs`**

Add to `backend/crates/sober-cli/src/commands/mod.rs`:

```rust
pub mod gc;
```

- [ ] **Step 4: Wire up in `sober.rs`**

In `backend/crates/sober-cli/src/sober.rs`, add the match arm in `main()`:

```rust
Command::Gc(cmd) => commands::gc::handle(cmd).await,
```

Add `GcCommand` to the imports:

```rust
use cli::{
    Cli, Command, ConfigCommand, EvolutionCommand, GcCommand, MigrateCommand,
    PluginCommand, SkillCommand, UserCommand,
};
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build -q -p sober-cli`
Expected: success

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-cli/src/commands/gc.rs \
       backend/crates/sober-cli/src/commands/mod.rs \
       backend/crates/sober-cli/src/cli.rs \
       backend/crates/sober-cli/src/sober.rs
git commit -m "feat(cli): add sober gc blobs command"
```

---

### Task 6: Fix plugin cache eviction on uninstall

**Files:**
- Modify: `backend/crates/sober-plugin/src/manager.rs`
- Modify: `backend/crates/sober-agent/src/grpc/plugins.rs`

- [ ] **Step 1: Add `uninstall` method to `PluginManager`**

In `backend/crates/sober-plugin/src/manager.rs`, add after the `evict_wasm_host`
method (around line 691):

```rust
/// Uninstalls a plugin: evicts the WASM host cache, then deletes the DB row.
///
/// Always use this instead of calling `registry().uninstall()` directly —
/// direct deletion leaves stale WASM hosts in the cache.
pub async fn uninstall(&self, id: PluginId) -> Result<(), PluginError> {
    self.evict_wasm_host(&id);
    self.registry.uninstall(id).await
}
```

- [ ] **Step 2: Update gRPC handler to use PluginManager**

In `backend/crates/sober-agent/src/grpc/plugins.rs`, change
`handle_uninstall_plugin` to use `PluginManager::uninstall` instead of
calling the repo directly:

```rust
pub(crate) async fn handle_uninstall_plugin<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    request: Request<proto::UninstallPluginRequest>,
) -> Result<Response<proto::UninstallPluginResponse>, Status> {
    let req = request.into_inner();

    let plugin_id = req
        .plugin_id
        .parse::<uuid::Uuid>()
        .map(PluginId::from_uuid)
        .map_err(|_| Status::invalid_argument("invalid plugin_id"))?;

    service
        .agent()
        .plugin_manager()
        .uninstall(plugin_id)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    info!(%plugin_id, "plugin uninstalled");

    Ok(Response::new(proto::UninstallPluginResponse {}))
}
```

Note: verify the accessor method name — it may be `plugin_manager()` or
similar. Check how `handle_uninstall_plugin` currently accesses the agent and
use the same pattern to reach the `PluginManager`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -q --workspace`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-plugin/src/manager.rs \
       backend/crates/sober-agent/src/grpc/plugins.rs
git commit -m "fix(plugin): evict WASM cache on uninstall"
```

---

### Task 7: Run sqlx prepare and full verification

**Files:**
- Modify: `backend/.sqlx/` (generated)

- [ ] **Step 1: Run sqlx prepare for offline mode**

```bash
cd backend && cargo sqlx prepare --workspace -q
```

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -q --workspace -- -D warnings`
Expected: no warnings

- [ ] **Step 3: Run all tests**

Run: `cargo test --workspace -q`
Expected: all pass

- [ ] **Step 4: Commit sqlx data if changed**

```bash
git add backend/.sqlx/
git commit -m "chore: update sqlx offline data for blob GC queries"
```

---

### Task 8: Update documentation

**Files:**
- Modify: `ARCHITECTURE.md`

- [ ] **Step 1: Update sober-scheduler description in crate map**

In `ARCHITECTURE.md`, update the `sober-scheduler` crate map row to mention
blob GC. Change:

```
| `sober-scheduler` | Autonomous tick engine, interval + cron scheduling, job persistence, local execution of deterministic jobs (artifact/internal) via executor registry. Depends on `sober-memory`, `sober-sandbox`, `sober-workspace` for local executors. |
```

To:

```
| `sober-scheduler` | Autonomous tick engine, interval + cron scheduling, job persistence, local execution of deterministic jobs (artifact/internal/blob GC) via executor registry. Depends on `sober-memory`, `sober-sandbox`, `sober-workspace` for local executors. |
```

- [ ] **Step 2: Check if mdBook docs need updates**

Check `docs/book/` for any mentions of blob storage that should reference GC.
If blob storage is documented, add a brief mention that orphaned blobs are
periodically collected by the scheduler.

- [ ] **Step 3: Commit**

```bash
git add ARCHITECTURE.md
git commit -m "docs: document blob GC in architecture"
```

---

### Task 9: Close plan

**Files:**
- Move: `docs/plans/pending/034-blob-gc/` → `docs/plans/done/034-blob-gc/`

- [ ] **Step 1: Move plan to done**

```bash
git mv docs/plans/pending/034-blob-gc docs/plans/done/034-blob-gc
```

- [ ] **Step 2: Final verification**

```bash
cargo clippy -q --workspace -- -D warnings && cargo test --workspace -q
```

- [ ] **Step 3: Commit**

```bash
git add docs/plans/
git commit -m "chore: close plan #034 blob GC"
```
