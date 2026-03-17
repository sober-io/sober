//! PostgreSQL implementation of [`ConversationUserRepo`].

use sober_core::{
    error::AppError,
    types::{
        ConversationId, ConversationUser, ConversationUserRepo, ConversationUserRole,
        ConversationUserWithUsername, UserId,
    },
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::{ConversationUserRow, ConversationUserWithUsernameRow};

/// PostgreSQL-backed conversation user membership repository.
pub struct PgConversationUserRepo {
    pool: PgPool,
}

impl PgConversationUserRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl ConversationUserRepo for PgConversationUserRepo {
    async fn create(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        role: ConversationUserRole,
    ) -> Result<ConversationUser, AppError> {
        let row = sqlx::query_as::<_, ConversationUserRow>(
            "INSERT INTO conversation_users (conversation_id, user_id, role) \
             VALUES ($1, $2, $3) \
             RETURNING conversation_id, user_id, unread_count, last_read_at, role, joined_at",
        )
        .bind(conversation_id.as_uuid())
        .bind(user_id.as_uuid())
        .bind(role)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn mark_read(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE conversation_users \
             SET unread_count = 0, last_read_at = now() \
             WHERE conversation_id = $1 AND user_id = $2",
        )
        .bind(conversation_id.as_uuid())
        .bind(user_id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn increment_unread(
        &self,
        conversation_id: ConversationId,
        exclude_user_id: UserId,
    ) -> Result<Vec<(UserId, i32)>, AppError> {
        #[derive(sqlx::FromRow)]
        struct UnreadRow {
            user_id: Uuid,
            unread_count: i32,
        }

        let rows = sqlx::query_as::<_, UnreadRow>(
            "UPDATE conversation_users \
             SET unread_count = unread_count + 1 \
             WHERE conversation_id = $1 AND user_id != $2 \
             RETURNING user_id, unread_count",
        )
        .bind(conversation_id.as_uuid())
        .bind(exclude_user_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows
            .into_iter()
            .map(|r| (UserId::from_uuid(r.user_id), r.unread_count))
            .collect())
    }

    async fn get(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> Result<ConversationUser, AppError> {
        let row = sqlx::query_as::<_, ConversationUserRow>(
            "SELECT conversation_id, user_id, unread_count, last_read_at, role, joined_at \
             FROM conversation_users \
             WHERE conversation_id = $1 AND user_id = $2",
        )
        .bind(conversation_id.as_uuid())
        .bind(user_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("conversation_user".into()))?;

        Ok(row.into())
    }

    async fn list_by_conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<Vec<ConversationUser>, AppError> {
        let rows = sqlx::query_as::<_, ConversationUserRow>(
            "SELECT conversation_id, user_id, unread_count, last_read_at, role, joined_at \
             FROM conversation_users \
             WHERE conversation_id = $1",
        )
        .bind(conversation_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn reset_all_unread(&self, conversation_id: ConversationId) -> Result<(), AppError> {
        sqlx::query("UPDATE conversation_users SET unread_count = 0 WHERE conversation_id = $1")
            .bind(conversation_id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn list_collaborators(
        &self,
        conversation_id: ConversationId,
    ) -> Result<Vec<ConversationUserWithUsername>, AppError> {
        let rows = sqlx::query_as::<_, ConversationUserWithUsernameRow>(
            "SELECT cu.conversation_id, cu.user_id, u.username, \
             cu.unread_count, cu.last_read_at, cu.role, cu.joined_at \
             FROM conversation_users cu \
             JOIN users u ON cu.user_id = u.id \
             WHERE cu.conversation_id = $1 \
             ORDER BY cu.joined_at",
        )
        .bind(conversation_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn update_role(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        role: ConversationUserRole,
    ) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE conversation_users SET role = $3 \
             WHERE conversation_id = $1 AND user_id = $2",
        )
        .bind(conversation_id.as_uuid())
        .bind(user_id.as_uuid())
        .bind(role)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("conversation_user".into()));
        }

        Ok(())
    }

    async fn remove_collaborator(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> Result<(), AppError> {
        let result = sqlx::query(
            "DELETE FROM conversation_users \
             WHERE conversation_id = $1 AND user_id = $2",
        )
        .bind(conversation_id.as_uuid())
        .bind(user_id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("conversation_user".into()));
        }

        Ok(())
    }
}
