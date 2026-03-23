//! PostgreSQL implementation of [`SandboxExecutionLogRepo`].

use sober_core::error::AppError;
use sober_core::types::CreateSandboxExecutionLog;
use sqlx::PgPool;

/// PostgreSQL-backed sandbox execution log repository.
#[derive(Clone)]
pub struct PgSandboxExecutionLogRepo {
    pool: PgPool,
}

impl PgSandboxExecutionLogRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::SandboxExecutionLogRepo for PgSandboxExecutionLogRepo {
    async fn create(&self, entry: CreateSandboxExecutionLog) -> Result<(), AppError> {
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
    }
}
