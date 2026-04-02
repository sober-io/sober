//! Memory pruning executor — removes low-importance memories that have decayed
//! below the configured threshold.

use std::sync::Arc;

use sober_core::config::MemoryConfig;
use sober_core::error::AppError;
use sober_core::types::{Job, UserId};
use sober_memory::MemoryStore;
use tracing::{info, instrument};

use crate::executor::{ExecutionResult, JobExecutor};

/// Prunes stale vector memories for a given user (or all users).
pub struct MemoryPruningExecutor {
    memory_store: Arc<MemoryStore>,
    memory_config: MemoryConfig,
}

impl MemoryPruningExecutor {
    /// Create a new memory pruning executor.
    pub fn new(memory_store: Arc<MemoryStore>, memory_config: MemoryConfig) -> Self {
        Self {
            memory_store,
            memory_config,
        }
    }
}

#[tonic::async_trait]
impl JobExecutor for MemoryPruningExecutor {
    #[instrument(skip(self, job), fields(job.id = %job.id, job.name = %job.name))]
    async fn execute(&self, job: &Job) -> Result<ExecutionResult, AppError> {
        // Extract user_id from the job's owner_id (system-wide prune if absent).
        let user_id = job
            .owner_id
            .map(UserId::from_uuid)
            .ok_or_else(|| AppError::Validation("memory_pruning requires owner_id".into()))?;

        let pruned = self
            .memory_store
            .prune(user_id, &self.memory_config)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        info!(user_id = %user_id, pruned, "memory pruning complete");

        Ok(ExecutionResult {
            summary: format!("pruned {pruned} memories for user {user_id}"),
            artifact_ref: None,
        })
    }
}
