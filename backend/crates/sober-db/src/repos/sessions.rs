//! PostgreSQL implementation of [`SessionRepo`].

use sober_core::error::AppError;
use sober_core::types::{CreateSession, Session};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::SessionRow;

/// PostgreSQL-backed session repository.
pub struct PgSessionRepo {
    pool: PgPool,
}

impl PgSessionRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::SessionRepo for PgSessionRepo {
    async fn get_by_token_hash(&self, token_hash: &str) -> Result<Option<Session>, AppError> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, user_id, token_hash, expires_at, created_at \
             FROM sessions WHERE token_hash = $1 AND expires_at > now()",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.map(Into::into))
    }

    async fn create(&self, input: CreateSession) -> Result<Session, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, SessionRow>(
            "INSERT INTO sessions (id, user_id, token_hash, expires_at) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id, user_id, token_hash, expires_at, created_at",
        )
        .bind(id)
        .bind(input.user_id.as_uuid())
        .bind(&input.token_hash)
        .bind(input.expires_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn delete_by_token_hash(&self, token_hash: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(token_hash)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn cleanup_expired(&self) -> Result<u64, AppError> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= now()")
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(result.rows_affected())
    }
}
