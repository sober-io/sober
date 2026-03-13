//! Session cleanup executor — deletes expired sessions from PostgreSQL.

use sober_core::error::AppError;
use sober_core::types::{Job, SessionRepo};
use tracing::info;

use crate::executor::{ExecutionResult, JobExecutor};

/// Deletes expired sessions from the database.
pub struct SessionCleanupExecutor<S: SessionRepo> {
    session_repo: S,
}

impl<S: SessionRepo> SessionCleanupExecutor<S> {
    /// Create a new session cleanup executor.
    pub fn new(session_repo: S) -> Self {
        Self { session_repo }
    }
}

#[tonic::async_trait]
impl<S: SessionRepo + 'static> JobExecutor for SessionCleanupExecutor<S> {
    async fn execute(&self, _job: &Job) -> Result<ExecutionResult, AppError> {
        let deleted = self.session_repo.cleanup_expired().await?;

        info!(deleted, "session cleanup complete");

        Ok(ExecutionResult {
            summary: format!("cleaned up {deleted} expired sessions"),
            artifact_ref: None,
        })
    }
}
