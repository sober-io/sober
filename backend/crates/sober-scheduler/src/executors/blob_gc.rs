//! Blob garbage collection executor — deletes orphaned blobs from the store.
//!
//! Walks the blob store in batches and queries the DB per batch to find
//! unreferenced keys. This scales better than loading all referenced keys
//! into a HashSet.

use std::sync::Arc;
use std::time::Duration;

use sober_core::error::AppError;
use sober_core::types::Job;
use sober_core::types::repo::BlobGcRepo;
use sober_workspace::BlobStore;
use tracing::{info, warn};

use crate::executor::{ExecutionResult, JobExecutor};

/// System job name registered in [`crate::system_jobs`].
pub const JOB_NAME: &str = "system::blob_gc";

/// Executor operation key in [`crate::executor::JobExecutorRegistry`].
pub const OP: &str = "blob_gc";

/// Default grace period: blobs younger than this are never deleted.
const DEFAULT_GRACE_PERIOD: Duration = Duration::from_secs(3600);

/// Batch size for filesystem walk.
const BATCH_SIZE: usize = 100;

/// Deletes orphaned blobs not referenced by any attachment, plugin, or artifact.
pub struct BlobGcExecutor<G: BlobGcRepo> {
    blob_store: Arc<BlobStore>,
    gc_repo: G,
    grace_period: Duration,
}

impl<G: BlobGcRepo> BlobGcExecutor<G> {
    /// Create a new blob GC executor.
    pub fn new(blob_store: Arc<BlobStore>, gc_repo: G) -> Self {
        Self {
            blob_store,
            gc_repo,
            grace_period: DEFAULT_GRACE_PERIOD,
        }
    }
}

#[tonic::async_trait]
impl<G: BlobGcRepo + 'static> JobExecutor for BlobGcExecutor<G> {
    async fn execute(&self, _job: &Job) -> Result<ExecutionResult, AppError> {
        let batches = self
            .blob_store
            .list_keys_batched(BATCH_SIZE, self.grace_period)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let mut scanned = 0usize;
        let mut deleted = 0u64;
        let mut bytes_freed = 0u64;
        let mut errors = Vec::new();

        for batch in &batches {
            scanned += batch.len();
            let orphans = self.gc_repo.find_unreferenced(batch).await?;

            for key in &orphans {
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
