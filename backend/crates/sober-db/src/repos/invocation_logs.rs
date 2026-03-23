//! PostgreSQL implementation of [`PluginInvocationLogRepo`].

use sober_core::error::AppError;
use sober_core::types::CreatePluginInvocationLog;
use sqlx::PgPool;

/// PostgreSQL-backed plugin invocation log repository.
#[derive(Clone)]
pub struct PgPluginInvocationLogRepo {
    pool: PgPool,
}

impl PgPluginInvocationLogRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::PluginInvocationLogRepo for PgPluginInvocationLogRepo {
    async fn create(&self, entry: CreatePluginInvocationLog) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO plugin_invocation_logs \
             (plugin_id, plugin_name, tool_name, user_id, conversation_id, \
              duration_ms, success, error_message) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(entry.plugin_id.map(|id| *id.as_uuid()))
        .bind(&entry.plugin_name)
        .bind(&entry.tool_name)
        .bind(entry.user_id.map(|id| *id.as_uuid()))
        .bind(entry.conversation_id.map(|id| *id.as_uuid()))
        .bind(entry.duration_ms)
        .bind(entry.success)
        .bind(&entry.error_message)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }
}
