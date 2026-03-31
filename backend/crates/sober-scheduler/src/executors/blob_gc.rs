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
    pub fn new(blob_store: Arc<BlobStore>, plugin_repo: P, artifact_repo: A) -> Self {
        Self {
            blob_store,
            plugin_repo,
            artifact_repo,
            grace_period: DEFAULT_GRACE_PERIOD,
        }
    }
}

#[tonic::async_trait]
impl<P: PluginRepo + 'static, A: ArtifactRepo + 'static> JobExecutor for BlobGcExecutor<P, A> {
    async fn execute(&self, _job: &Job) -> Result<ExecutionResult, AppError> {
        // 1. List all blobs on disk.
        let all_blobs = self
            .blob_store
            .list_keys()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
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
