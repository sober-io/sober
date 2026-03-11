//! PostgreSQL implementation of [`ConversationRepo`].

use sober_core::error::AppError;
use sober_core::types::{Conversation, ConversationId, UserId, WorkspaceId};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::ConversationRow;

/// PostgreSQL-backed conversation repository.
pub struct PgConversationRepo {
    pool: PgPool,
}

impl PgConversationRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::ConversationRepo for PgConversationRepo {
    async fn create(
        &self,
        user_id: UserId,
        title: Option<&str>,
        workspace_id: Option<WorkspaceId>,
    ) -> Result<Conversation, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, ConversationRow>(
            "INSERT INTO conversations (id, user_id, title, workspace_id) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id, user_id, title, workspace_id, created_at, updated_at",
        )
        .bind(id)
        .bind(user_id.as_uuid())
        .bind(title)
        .bind(workspace_id.map(|w| *w.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn get_by_id(&self, id: ConversationId) -> Result<Conversation, AppError> {
        let row = sqlx::query_as::<_, ConversationRow>(
            "SELECT id, user_id, title, workspace_id, created_at, updated_at \
             FROM conversations WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("conversation".into()))?;

        Ok(row.into())
    }

    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<Conversation>, AppError> {
        let rows = sqlx::query_as::<_, ConversationRow>(
            "SELECT id, user_id, title, workspace_id, created_at, updated_at \
             FROM conversations WHERE user_id = $1 \
             ORDER BY updated_at DESC",
        )
        .bind(user_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn update_title(&self, id: ConversationId, title: &str) -> Result<(), AppError> {
        let result =
            sqlx::query("UPDATE conversations SET title = $1, updated_at = now() WHERE id = $2")
                .bind(title)
                .bind(id.as_uuid())
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("conversation".into()));
        }

        Ok(())
    }

    async fn delete(&self, id: ConversationId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM conversations WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("conversation".into()));
        }

        Ok(())
    }
}
