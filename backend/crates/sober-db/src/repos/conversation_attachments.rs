//! PostgreSQL implementation of [`ConversationAttachmentRepo`].

use sober_core::error::AppError;
use sober_core::types::{
    ConversationAttachment, ConversationAttachmentId, ConversationId, CreateConversationAttachment,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::ConversationAttachmentRow;

/// Column list for conversation attachment queries.
const COLS: &str = "id, blob_key, kind, content_type, filename, size, metadata, \
                    conversation_id, user_id, created_at";

/// PostgreSQL-backed conversation attachment repository.
#[derive(Clone)]
pub struct PgConversationAttachmentRepo {
    pool: PgPool,
}

impl PgConversationAttachmentRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::ConversationAttachmentRepo for PgConversationAttachmentRepo {
    async fn create(
        &self,
        input: CreateConversationAttachment,
    ) -> Result<ConversationAttachment, AppError> {
        let id = Uuid::now_v7();
        let query = format!(
            "INSERT INTO conversation_attachments \
             (id, blob_key, kind, content_type, filename, size, metadata, conversation_id, user_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
             RETURNING {COLS}"
        );
        let row = sqlx::query_as::<_, ConversationAttachmentRow>(&query)
            .bind(id)
            .bind(&input.blob_key)
            .bind(input.kind)
            .bind(&input.content_type)
            .bind(&input.filename)
            .bind(input.size)
            .bind(&input.metadata)
            .bind(input.conversation_id.as_uuid())
            .bind(input.user_id.as_uuid())
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn get_by_id(
        &self,
        id: ConversationAttachmentId,
    ) -> Result<ConversationAttachment, AppError> {
        let query = format!("SELECT {COLS} FROM conversation_attachments WHERE id = $1");
        let row = sqlx::query_as::<_, ConversationAttachmentRow>(&query)
            .bind(id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?
            .ok_or_else(|| AppError::NotFound("attachment".into()))?;

        Ok(row.into())
    }

    async fn list_by_conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<Vec<ConversationAttachment>, AppError> {
        let query = format!(
            "SELECT {COLS} FROM conversation_attachments \
             WHERE conversation_id = $1 \
             ORDER BY created_at ASC"
        );
        let rows = sqlx::query_as::<_, ConversationAttachmentRow>(&query)
            .bind(conversation_id.as_uuid())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_by_ids(
        &self,
        ids: &[ConversationAttachmentId],
    ) -> Result<Vec<ConversationAttachment>, AppError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let uuids: Vec<Uuid> = ids.iter().map(|id| *id.as_uuid()).collect();
        let query = format!("SELECT {COLS} FROM conversation_attachments WHERE id = ANY($1)");
        let rows = sqlx::query_as::<_, ConversationAttachmentRow>(&query)
            .bind(&uuids)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn delete(&self, id: ConversationAttachmentId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM conversation_attachments WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("attachment".into()));
        }

        Ok(())
    }

    async fn delete_orphaned(&self, max_age: std::time::Duration) -> Result<u64, AppError> {
        let interval = pg_interval(max_age);
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "DELETE FROM conversation_attachments ca \
             WHERE ca.created_at < NOW() - $1::interval \
               AND NOT EXISTS ( \
                   SELECT 1 FROM conversation_messages cm \
                   WHERE cm.conversation_id = ca.conversation_id \
                     AND EXISTS ( \
                         SELECT 1 FROM jsonb_array_elements(cm.content) AS elem \
                         WHERE elem->>'conversation_attachment_id' = ca.id::text \
                     ) \
               ) \
             RETURNING ca.id",
        )
        .bind(&interval)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.len() as u64)
    }

    async fn find_unreferenced_by_message(
        &self,
        conversation_attachment_ids: &[ConversationAttachmentId],
        conversation_id: ConversationId,
    ) -> Result<Vec<ConversationAttachmentId>, AppError> {
        if conversation_attachment_ids.is_empty() {
            return Ok(Vec::new());
        }
        let uuids: Vec<Uuid> = conversation_attachment_ids
            .iter()
            .map(|id| *id.as_uuid())
            .collect();
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT a.id FROM unnest($1::uuid[]) AS a(id) \
             WHERE NOT EXISTS ( \
                 SELECT 1 FROM conversation_messages cm \
                 WHERE cm.conversation_id = $2 \
                   AND EXISTS ( \
                       SELECT 1 FROM jsonb_array_elements(cm.content) AS elem \
                       WHERE elem->>'conversation_attachment_id' = a.id::text \
                   ) \
             )",
        )
        .bind(&uuids)
        .bind(conversation_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows
            .into_iter()
            .map(|(id,)| ConversationAttachmentId::from_uuid(id))
            .collect())
    }
}

/// Converts a [`std::time::Duration`] into a PostgreSQL interval string.
fn pg_interval(d: std::time::Duration) -> String {
    format!("{} seconds", d.as_secs())
}
