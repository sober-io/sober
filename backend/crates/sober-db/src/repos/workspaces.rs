//! PostgreSQL implementation of [`WorkspaceRepo`].

use sober_core::error::AppError;
use sober_core::types::{UserId, Workspace, WorkspaceId};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::WorkspaceRow;

/// PostgreSQL-backed workspace repository.
pub struct PgWorkspaceRepo {
    pool: PgPool,
}

impl PgWorkspaceRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::WorkspaceRepo for PgWorkspaceRepo {
    async fn create(
        &self,
        user_id: UserId,
        name: &str,
        root_path: &str,
    ) -> Result<Workspace, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, WorkspaceRow>(
            "INSERT INTO workspaces (id, user_id, name, root_path) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id, user_id, name, root_path, archived, created_at, updated_at",
        )
        .bind(id)
        .bind(user_id.as_uuid())
        .bind(name)
        .bind(root_path)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn get_by_id(&self, id: WorkspaceId) -> Result<Workspace, AppError> {
        let row = sqlx::query_as::<_, WorkspaceRow>(
            "SELECT id, user_id, name, root_path, archived, created_at, updated_at \
             FROM workspaces WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("workspace".into()))?;

        Ok(row.into())
    }

    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<Workspace>, AppError> {
        let rows = sqlx::query_as::<_, WorkspaceRow>(
            "SELECT id, user_id, name, root_path, archived, created_at, updated_at \
             FROM workspaces WHERE user_id = $1 AND archived = false \
             ORDER BY updated_at DESC",
        )
        .bind(user_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn archive(&self, id: WorkspaceId) -> Result<(), AppError> {
        let result =
            sqlx::query("UPDATE workspaces SET archived = true, updated_at = now() WHERE id = $1")
                .bind(id.as_uuid())
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("workspace".into()));
        }

        Ok(())
    }

    async fn restore(&self, id: WorkspaceId) -> Result<(), AppError> {
        let result =
            sqlx::query("UPDATE workspaces SET archived = false, updated_at = now() WHERE id = $1")
                .bind(id.as_uuid())
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("workspace".into()));
        }

        Ok(())
    }

    async fn delete(&self, id: WorkspaceId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM workspaces WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("workspace".into()));
        }

        Ok(())
    }
}
