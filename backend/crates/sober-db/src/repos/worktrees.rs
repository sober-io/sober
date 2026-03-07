//! PostgreSQL implementation of [`WorktreeRepo`].

use chrono::{DateTime, Utc};
use sober_core::error::AppError;
use sober_core::types::{WorkspaceRepoId, Worktree, WorktreeId};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::WorktreeRow;

/// PostgreSQL-backed worktree repository.
pub struct PgWorktreeRepo {
    pool: PgPool,
}

impl PgWorktreeRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::WorktreeRepo for PgWorktreeRepo {
    async fn create(
        &self,
        repo_id: WorkspaceRepoId,
        branch: &str,
        path: &str,
    ) -> Result<Worktree, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, WorktreeRow>(
            "INSERT INTO worktrees (id, repo_id, branch, path) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id, repo_id, branch, path, stale, created_at",
        )
        .bind(id)
        .bind(repo_id.as_uuid())
        .bind(branch)
        .bind(path)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn list_by_repo(
        &self,
        repo_id: WorkspaceRepoId,
    ) -> Result<Vec<Worktree>, AppError> {
        let rows = sqlx::query_as::<_, WorktreeRow>(
            "SELECT id, repo_id, branch, path, stale, created_at \
             FROM worktrees WHERE repo_id = $1 \
             ORDER BY created_at DESC",
        )
        .bind(repo_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_stale(&self, older_than: DateTime<Utc>) -> Result<Vec<Worktree>, AppError> {
        let rows = sqlx::query_as::<_, WorktreeRow>(
            "SELECT id, repo_id, branch, path, stale, created_at \
             FROM worktrees WHERE stale = false AND created_at < $1 \
             ORDER BY created_at ASC",
        )
        .bind(older_than)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn mark_stale(&self, id: WorktreeId) -> Result<(), AppError> {
        let result = sqlx::query("UPDATE worktrees SET stale = true WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("worktree".into()));
        }

        Ok(())
    }

    async fn delete(&self, id: WorktreeId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM worktrees WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("worktree".into()));
        }

        Ok(())
    }
}
