//! PostgreSQL implementation of [`SandboxExecutionLogRepo`].

use std::pin::Pin;

use sober_core::error::AppError;
use sober_core::types::CreateSandboxExecutionLog;
use sqlx::PgPool;

/// PostgreSQL-backed sandbox execution log repository.
#[derive(Clone)]
pub struct PgSandboxExecutionLogRepo {
    pool: PgPool,
}

impl PgSandboxExecutionLogRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::SandboxExecutionLogRepo for PgSandboxExecutionLogRepo {
    fn create(
        &self,
        entry: CreateSandboxExecutionLog,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), AppError>> + Send + '_>> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO sandbox_execution_logs \
                 (execution_id, workspace_id, user_id, policy_name, command, trigger, \
                  duration_ms, exit_code, denied_network_requests, outcome) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            )
            .bind(entry.execution_id)
            .bind(entry.workspace_id.map(|id| *id.as_uuid()))
            .bind(entry.user_id.map(|id| *id.as_uuid()))
            .bind(&entry.policy_name)
            .bind(&entry.command)
            .bind(&entry.trigger)
            .bind(entry.duration_ms)
            .bind(entry.exit_code)
            .bind(&entry.denied_network_requests)
            .bind(&entry.outcome)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

            Ok(())
        })
    }
}
