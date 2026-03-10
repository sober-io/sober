//! PostgreSQL implementation of [`MessageRepo`].

use sober_core::error::AppError;
use sober_core::types::{ConversationId, CreateMessage, Message};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::MessageRow;

/// PostgreSQL-backed message repository.
pub struct PgMessageRepo {
    pool: PgPool,
}

impl PgMessageRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::MessageRepo for PgMessageRepo {
    async fn create(&self, input: CreateMessage) -> Result<Message, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, MessageRow>(
            "INSERT INTO messages (id, conversation_id, role, content, tool_calls, tool_result, token_count) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING id, conversation_id, role, content, tool_calls, tool_result, token_count, created_at",
        )
        .bind(id)
        .bind(input.conversation_id.as_uuid())
        .bind(input.role)
        .bind(&input.content)
        .bind(&input.tool_calls)
        .bind(&input.tool_result)
        .bind(input.token_count)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn list_by_conversation(
        &self,
        conversation_id: ConversationId,
        limit: i64,
    ) -> Result<Vec<Message>, AppError> {
        // Fetch the most recent N messages, then reverse to chronological order
        // so older messages appear first in the conversation context.
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT * FROM (\
                SELECT id, conversation_id, role, content, tool_calls, tool_result, token_count, created_at \
                FROM messages WHERE conversation_id = $1 \
                ORDER BY created_at DESC \
                LIMIT $2\
             ) AS recent ORDER BY created_at ASC",
        )
        .bind(conversation_id.as_uuid())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }
}
