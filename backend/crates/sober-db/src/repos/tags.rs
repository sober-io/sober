//! PostgreSQL implementation of [`TagRepo`].

use sober_core::{
    error::AppError,
    types::{ConversationId, CreateTag, MessageId, Tag, TagId, TagRepo, UserId},
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::TagRow;

/// Base hues (degrees) for the tag color palette.
const BASE_HUES: &[u16] = &[0, 25, 45, 145, 190, 220, 260, 330];

/// Generates a deterministic HSL color from a tag name.
///
/// The name hash selects a base hue, then varies saturation (65-85%)
/// and lightness (45-60%) so tags sharing a hue bucket still look distinct.
fn color_for_name(name: &str) -> String {
    let hash = name.bytes().fold(0u32, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(u32::from(b))
    });
    let hue = BASE_HUES[(hash as usize) % BASE_HUES.len()];
    // Use different bits of the hash for saturation and lightness variation.
    let sat = 50 + ((hash >> 8) % 41); // 50–90
    let lit = 35 + ((hash >> 16) % 31); // 35–65
    format!("hsl({hue}, {sat}%, {lit}%)")
}

/// PostgreSQL-backed tag repository.
pub struct PgTagRepo {
    pool: PgPool,
}

impl PgTagRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl TagRepo for PgTagRepo {
    async fn create_or_get(&self, input: CreateTag) -> Result<Tag, AppError> {
        // Attempt insert; on conflict (user_id, name) do nothing, then select.
        sqlx::query(
            "INSERT INTO tags (id, user_id, name, color) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (user_id, name) DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(input.user_id.as_uuid())
        .bind(&input.name)
        .bind(color_for_name(&input.name))
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        let row = sqlx::query_as::<_, TagRow>(
            "SELECT id, user_id, name, color, created_at \
             FROM tags WHERE user_id = $1 AND name = $2",
        )
        .bind(input.user_id.as_uuid())
        .bind(&input.name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::Internal("tag missing after upsert".into()))?;

        Ok(row.into())
    }

    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<Tag>, AppError> {
        let rows = sqlx::query_as::<_, TagRow>(
            "SELECT id, user_id, name, color, created_at \
             FROM tags WHERE user_id = $1 ORDER BY name",
        )
        .bind(user_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn tag_conversation(
        &self,
        conversation_id: ConversationId,
        tag_id: TagId,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO conversation_tags (conversation_id, tag_id) \
             VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(conversation_id.as_uuid())
        .bind(tag_id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn untag_conversation(
        &self,
        conversation_id: ConversationId,
        tag_id: TagId,
    ) -> Result<(), AppError> {
        sqlx::query("DELETE FROM conversation_tags WHERE conversation_id = $1 AND tag_id = $2")
            .bind(conversation_id.as_uuid())
            .bind(tag_id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn tag_message(&self, message_id: MessageId, tag_id: TagId) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO message_tags (message_id, tag_id) \
             VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(message_id.as_uuid())
        .bind(tag_id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn untag_message(&self, message_id: MessageId, tag_id: TagId) -> Result<(), AppError> {
        sqlx::query("DELETE FROM message_tags WHERE message_id = $1 AND tag_id = $2")
            .bind(message_id.as_uuid())
            .bind(tag_id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn list_by_conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<Vec<Tag>, AppError> {
        let rows = sqlx::query_as::<_, TagRow>(
            "SELECT t.id, t.user_id, t.name, t.color, t.created_at \
             FROM tags t \
             JOIN conversation_tags ct ON t.id = ct.tag_id \
             WHERE ct.conversation_id = $1 \
             ORDER BY t.name",
        )
        .bind(conversation_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }
}
