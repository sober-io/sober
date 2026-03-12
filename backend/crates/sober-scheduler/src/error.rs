//! Scheduler error types.

/// Errors produced by scheduler operations.
#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    /// Invalid cron expression.
    #[error("invalid cron expression: {0}")]
    InvalidCron(String),

    /// Invalid interval specification.
    #[error("invalid interval: {0}")]
    InvalidInterval(String),

    /// Job not found.
    #[error("job not found: {0}")]
    NotFound(String),

    /// Database error.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    /// gRPC communication error.
    #[error("grpc error: {0}")]
    Grpc(#[from] tonic::Status),

    /// Internal error.
    #[error("{0}")]
    Internal(String),
}
