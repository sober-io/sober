//! PostgreSQL implementation of [`PluginExecutionLogRepo`].

use std::pin::Pin;

use sober_core::error::AppError;
use sober_core::types::CreatePluginExecutionLog;
use sqlx::PgPool;

/// PostgreSQL-backed plugin execution log repository.
#[derive(Clone)]
pub struct PgPluginExecutionLogRepo {
    pool: PgPool,
}

impl PgPluginExecutionLogRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::PluginExecutionLogRepo for PgPluginExecutionLogRepo {
    fn create(
        &self,
        entry: CreatePluginExecutionLog,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), AppError>> + Send + '_>> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO plugin_execution_logs \
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
        })
    }
}
