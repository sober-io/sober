//! Orphan collection cleanup executor — deletes `conv_` Qdrant collections
//! whose conversations no longer exist in Postgres.

use std::sync::Arc;

use sober_core::error::AppError;
use sober_core::types::{ConversationId, Job};
use sober_memory::MemoryStore;
use sober_memory::store::CONVERSATION_COLLECTION_PREFIX;
use tracing::{info, instrument, warn};

use crate::executor::{ExecutionResult, JobExecutor};

/// Operation key for job registration.
pub const OP: &str = "memory_orphan_cleanup";

/// Deletes conversation collections whose conversations no longer exist.
pub struct MemoryOrphanCleanupExecutor<C: sober_core::types::ConversationRepo> {
    memory_store: Arc<MemoryStore>,
    conversation_repo: Arc<C>,
}

impl<C: sober_core::types::ConversationRepo> MemoryOrphanCleanupExecutor<C> {
    /// Create a new orphan cleanup executor.
    pub fn new(memory_store: Arc<MemoryStore>, conversation_repo: Arc<C>) -> Self {
        Self {
            memory_store,
            conversation_repo,
        }
    }
}

#[tonic::async_trait]
impl<C: sober_core::types::ConversationRepo + 'static> JobExecutor
    for MemoryOrphanCleanupExecutor<C>
{
    #[instrument(skip(self, _job), fields(job.id = %_job.id, job.name = %_job.name))]
    async fn execute(&self, _job: &Job) -> Result<ExecutionResult, AppError> {
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

        let all_ids: Vec<ConversationId> = conv_collections.iter().map(|(_, id)| *id).collect();

        let existing = self.conversation_repo.filter_existing_ids(&all_ids).await?;

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
            deleted, "orphan collection cleanup complete"
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
