//! PostgreSQL implementation of [`MessageRepo`].

use metrics::counter;
use sober_core::error::AppError;
use sober_core::types::{
    ContentBlock, ConversationId, CreateMessage, Message, MessageId, MessageSearchHit, UserId,
};
use sqlx::PgPool;
use sqlx::postgres::PgConnection;
use uuid::Uuid;

use crate::rows::{MessageRow, MessageSearchHitRow};

/// Column list for message queries.
const MSG_COLUMNS: &str = "id, conversation_id, role, content, reasoning, \
                            token_count, user_id, metadata, created_at";

/// PostgreSQL-backed message repository.
#[derive(Clone)]
pub struct PgMessageRepo {
    pool: PgPool,
}

impl PgMessageRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Deletes a message on the given connection.
    pub async fn delete_tx(conn: &mut PgConnection, id: MessageId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM conversation_messages WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&mut *conn)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("message".into()));
        }

        Ok(())
    }

    /// Clears all messages in a conversation on the given connection.
    pub async fn clear_conversation_tx(
        conn: &mut PgConnection,
        conversation_id: ConversationId,
    ) -> Result<(), AppError> {
        sqlx::query("DELETE FROM conversation_messages WHERE conversation_id = $1")
            .bind(conversation_id.as_uuid())
            .execute(&mut *conn)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }
}

impl sober_core::types::MessageRepo for PgMessageRepo {
    async fn create(&self, input: CreateMessage) -> Result<Message, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, MessageRow>(
            &format!(
                "INSERT INTO conversation_messages (id, conversation_id, role, content, reasoning, token_count, metadata, user_id) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
                 RETURNING {MSG_COLUMNS}"
            ),
        )
        .bind(id)
        .bind(input.conversation_id.as_uuid())
        .bind(input.role)
        .bind(sqlx::types::Json(&input.content))
        .bind(&input.reasoning)
        .bind(input.token_count)
        .bind(&input.metadata)
        .bind(input.user_id.map(|u| *u.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        // Increment unread for all conversation members whose read position
        // is before this message (or NULL), excluding the message author.
        // This is the single centralized path for unread tracking — every
        // stored message goes through here.
        let exclude_author = input.user_id.map(|u| *u.as_uuid()).unwrap_or(Uuid::nil());
        sqlx::query(
            "UPDATE conversation_users \
             SET unread_count = unread_count + 1 \
             WHERE conversation_id = $1 \
               AND user_id != $3 \
               AND (last_read_message_id IS NULL OR last_read_message_id < $2)",
        )
        .bind(input.conversation_id.as_uuid())
        .bind(id)
        .bind(exclude_author)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        for block in &input.content {
            let block_type = match block {
                ContentBlock::Text { .. } => "text",
                ContentBlock::Image { .. } => "image",
                ContentBlock::File { .. } => "file",
                ContentBlock::Audio { .. } => "audio",
                ContentBlock::Video { .. } => "video",
            };
            counter!("sober_message_content_blocks_total", "type" => block_type).increment(1);
        }

        Ok(row.into())
    }

    async fn list_by_conversation(
        &self,
        conversation_id: ConversationId,
        limit: i64,
    ) -> Result<Vec<Message>, AppError> {
        // Fetch the most recent N messages, then reverse to chronological order
        // so older messages appear first in the conversation context.
        let rows = sqlx::query_as::<_, MessageRow>(&format!(
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

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_paginated(
        &self,
        conversation_id: ConversationId,
        before: Option<MessageId>,
        limit: i64,
    ) -> Result<Vec<Message>, AppError> {
        let rows = sqlx::query_as::<_, MessageRow>(&format!(
            "SELECT {MSG_COLUMNS} FROM conversation_messages \
                 WHERE conversation_id = $1 AND ($2::uuid IS NULL OR id < $2) \
                 ORDER BY id DESC \
                 LIMIT $3"
        ))
        .bind(conversation_id.as_uuid())
        .bind(before.map(|b| *b.as_uuid()))
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        // Reverse to chronological order.
        let mut messages: Vec<Message> = rows.into_iter().map(Into::into).collect();
        messages.reverse();
        Ok(messages)
    }

    async fn delete(&self, id: MessageId) -> Result<(), AppError> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        Self::delete_tx(&mut conn, id).await
    }

    async fn clear_conversation(&self, conversation_id: ConversationId) -> Result<(), AppError> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        Self::clear_conversation_tx(&mut conn, conversation_id).await
    }

    async fn get_by_id(&self, id: MessageId) -> Result<Message, AppError> {
        let row = sqlx::query_as::<_, MessageRow>(&format!(
            "SELECT {MSG_COLUMNS} FROM conversation_messages WHERE id = $1"
        ))
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("message".into()))?;

        Ok(row.into())
    }

    async fn update_content(
        &self,
        id: MessageId,
        content: &[ContentBlock],
        reasoning: Option<&str>,
    ) -> Result<(), AppError> {
        sqlx::query("UPDATE conversation_messages SET content = $2, reasoning = $3 WHERE id = $1")
            .bind(id.as_uuid())
            .bind(sqlx::types::Json(content))
            .bind(reasoning)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        Ok(())
    }

    async fn search_by_user(
        &self,
        user_id: UserId,
        query: &str,
        conversation_id: Option<ConversationId>,
        limit: i64,
    ) -> Result<Vec<MessageSearchHit>, AppError> {
        let rows = sqlx::query_as::<_, MessageSearchHitRow>(
            "SELECT m.id, m.conversation_id, c.title, m.role, m.content, m.created_at, \
             GREATEST( \
                 ts_rank_cd(to_tsvector('english', extract_message_text(m.content)), websearch_to_tsquery('english', $1)), \
                 ts_rank_cd(to_tsvector('simple', extract_message_text(m.content)), websearch_to_tsquery('simple', $1)) \
             ) AS rank \
             FROM conversation_messages m \
             JOIN conversations c ON c.id = m.conversation_id \
             WHERE c.user_id = $2 \
               AND (to_tsvector('english', extract_message_text(m.content)) @@ websearch_to_tsquery('english', $1) \
                    OR to_tsvector('simple', extract_message_text(m.content)) @@ websearch_to_tsquery('simple', $1)) \
               AND ($3::uuid IS NULL OR m.conversation_id = $3) \
             ORDER BY rank DESC \
             LIMIT $4",
        )
        .bind(query)
        .bind(user_id.as_uuid())
        .bind(conversation_id.map(|id| *id.as_uuid()))
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(MessageSearchHit::from).collect())
    }
}
