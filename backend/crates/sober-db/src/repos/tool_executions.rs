//! PostgreSQL implementation of [`ToolExecutionRepo`].

use std::collections::HashMap;

use sober_core::error::AppError;
use sober_core::types::tool_execution::MessageWithExecutions;
use sober_core::types::{
    ConversationId, CreateToolExecution, MessageId, ToolExecution, ToolExecutionId,
    ToolExecutionStatus,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::{MessageRow, ToolExecutionRow};

/// Column list for tool execution queries.
const EXEC_COLUMNS: &str = "id, conversation_id, conversation_message_id, tool_call_id, \
                             tool_name, input, source, status, output, error, plugin_id, \
                             created_at, started_at, completed_at";

/// Column list for message queries (matches messages.rs).
const MSG_COLUMNS: &str = "id, conversation_id, role, content, reasoning, \
                            token_count, user_id, metadata, created_at";

/// PostgreSQL-backed tool execution repository.
pub struct PgToolExecutionRepo {
    pool: PgPool,
}

impl PgToolExecutionRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::ToolExecutionRepo for PgToolExecutionRepo {
    async fn create_pending(&self, input: CreateToolExecution) -> Result<ToolExecution, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, ToolExecutionRow>(
            &format!(
                "INSERT INTO conversation_tool_executions \
                 (id, conversation_id, conversation_message_id, tool_call_id, tool_name, input, source, status, plugin_id) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', $8) \
                 RETURNING {EXEC_COLUMNS}"
            ),
        )
        .bind(id)
        .bind(input.conversation_id.as_uuid())
        .bind(input.conversation_message_id.as_uuid())
        .bind(&input.tool_call_id)
        .bind(&input.tool_name)
        .bind(&input.input)
        .bind(input.source)
        .bind(input.plugin_id.map(|p| *p.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn update_status(
        &self,
        id: ToolExecutionId,
        status: ToolExecutionStatus,
        output: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE conversation_tool_executions \
             SET status = $2, \
                 output = COALESCE($3, output), \
                 error = COALESCE($4, error), \
                 started_at = CASE \
                     WHEN $2 = 'running' AND started_at IS NULL THEN now() \
                     ELSE started_at \
                 END, \
                 completed_at = CASE \
                     WHEN $2 IN ('completed', 'failed', 'cancelled') THEN now() \
                     ELSE completed_at \
                 END \
             WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(status)
        .bind(output)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("tool_execution".into()));
        }

        Ok(())
    }

    async fn update_input(
        &self,
        id: ToolExecutionId,
        input: &serde_json::Value,
    ) -> Result<(), AppError> {
        let result =
            sqlx::query("UPDATE conversation_tool_executions SET input = $2 WHERE id = $1")
                .bind(id.as_uuid())
                .bind(input)
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("tool_execution".into()));
        }

        Ok(())
    }

    async fn find_incomplete(
        &self,
        conversation_id: ConversationId,
    ) -> Result<Vec<ToolExecution>, AppError> {
        let rows = sqlx::query_as::<_, ToolExecutionRow>(&format!(
            "SELECT {EXEC_COLUMNS} FROM conversation_tool_executions \
             WHERE conversation_id = $1 AND status IN ('pending', 'running') \
             ORDER BY created_at ASC"
        ))
        .bind(conversation_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn find_by_message(&self, message_id: MessageId) -> Result<Vec<ToolExecution>, AppError> {
        let rows = sqlx::query_as::<_, ToolExecutionRow>(&format!(
            "SELECT {EXEC_COLUMNS} FROM conversation_tool_executions \
             WHERE conversation_message_id = $1 \
             ORDER BY created_at ASC"
        ))
        .bind(message_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_messages_with_executions(
        &self,
        conversation_id: ConversationId,
        limit: i64,
    ) -> Result<Vec<MessageWithExecutions>, AppError> {
        // 1. Load recent messages (most recent N, then reverse to chronological order).
        let message_rows = sqlx::query_as::<_, MessageRow>(&format!(
            "SELECT * FROM (\
                 SELECT {MSG_COLUMNS} \
                 FROM conversation_messages WHERE conversation_id = $1 \
                 ORDER BY created_at DESC \
                 LIMIT $2\
             ) AS recent ORDER BY created_at ASC"
        ))
        .bind(conversation_id.as_uuid())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if message_rows.is_empty() {
            return Ok(Vec::new());
        }

        // 2. Collect message IDs.
        let message_ids: Vec<Uuid> = message_rows.iter().map(|r| r.id).collect();

        // 3. Batch-load all tool executions for those messages.
        let exec_rows = sqlx::query_as::<_, ToolExecutionRow>(&format!(
            "SELECT {EXEC_COLUMNS} FROM conversation_tool_executions \
             WHERE conversation_message_id = ANY($1) \
             ORDER BY created_at ASC"
        ))
        .bind(&message_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        // 4. Group executions by message_id.
        let mut exec_map: HashMap<Uuid, Vec<ToolExecution>> = HashMap::new();
        for row in exec_rows {
            let msg_id = row.conversation_message_id;
            exec_map.entry(msg_id).or_default().push(row.into());
        }

        // 5. Build MessageWithExecutions.
        let result = message_rows
            .into_iter()
            .map(|row| {
                let msg_id = row.id;
                let message = row.into();
                let tool_executions = exec_map.remove(&msg_id).unwrap_or_default();
                MessageWithExecutions {
                    message,
                    tool_executions,
                }
            })
            .collect();

        Ok(result)
    }
}
