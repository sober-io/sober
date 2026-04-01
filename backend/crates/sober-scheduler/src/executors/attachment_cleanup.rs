//! Attachment cleanup executor — deletes orphaned conversation attachments.
//!
//! An attachment is "orphaned" if it was uploaded but never referenced by
//! any message content block within 24 hours.

use std::time::Duration;

use sober_core::error::AppError;
use sober_core::types::ConversationAttachmentRepo;
use sober_core::types::Job;
use tracing::info;

use crate::executor::{ExecutionResult, JobExecutor};

/// System job name.
pub const JOB_NAME: &str = "system::attachment_cleanup";

/// Executor operation key.
pub const OP: &str = "attachment_cleanup";

/// Default max age for orphaned attachments (24 hours).
const DEFAULT_MAX_AGE: Duration = Duration::from_secs(86400);

/// Deletes conversation attachments not referenced by any message.
pub struct AttachmentCleanupExecutor<R: ConversationAttachmentRepo> {
    repo: R,
}

impl<R: ConversationAttachmentRepo> AttachmentCleanupExecutor<R> {
    /// Create a new attachment cleanup executor.
    pub fn new(repo: R) -> Self {
        Self { repo }
    }
}

#[tonic::async_trait]
impl<R: ConversationAttachmentRepo + 'static> JobExecutor for AttachmentCleanupExecutor<R> {
    async fn execute(&self, _job: &Job) -> Result<ExecutionResult, AppError> {
        let deleted = self.repo.delete_orphaned(DEFAULT_MAX_AGE).await?;

        metrics::counter!("sober_attachment_cleanup_deleted_total").increment(deleted);

        let summary = format!("attachment cleanup: deleted {deleted} orphaned attachments");
        info!("{summary}");

        Ok(ExecutionResult {
            summary,
            artifact_ref: None,
        })
    }
}
