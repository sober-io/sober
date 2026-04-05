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
        if let Some(conv_ids) = job
            .payload
            .get("conversation_ids")
            .and_then(|v| v.as_array())
        {
            for val in conv_ids {
                if let Some(id_str) = val.as_str()
                    && let Ok(uuid) = uuid::Uuid::parse_str(id_str)
                {
                    let conv_id = sober_core::ConversationId::from_uuid(uuid);
                    let stats = self
                        .memory_store
                        .deduplicate(CollectionTarget::Conversation(conv_id), &self.memory_config)
                        .await
                        .map_err(|e| AppError::Internal(e.into()))?;
                    conv_merged += stats.merged;
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
