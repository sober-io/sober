//! PostgreSQL implementation of [`WorktreeRepo`].

use chrono::{DateTime, Utc};
use sober_core::error::AppError;
use sober_core::types::{ConversationId, UserId, WorkspaceRepoId, Worktree, WorktreeId};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::WorktreeRow;

/// Column list for worktree queries.
const WORKTREE_COLS: &str = "id, repo_id, branch, path, state, created_by, \
                             task_id, conversation_id, created_at, last_active_at";

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
        created_by: Option<UserId>,
        task_id: Option<Uuid>,
        conversation_id: Option<ConversationId>,
    ) -> Result<Worktree, AppError> {
        let id = Uuid::now_v7();
        let query = format!(
            "INSERT INTO worktrees (id, repo_id, branch, path, created_by, task_id, conversation_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING {WORKTREE_COLS}"
        );
        let row = sqlx::query_as::<_, WorktreeRow>(&query)
            .bind(id)
            .bind(repo_id.as_uuid())
            .bind(branch)
            .bind(path)
            .bind(created_by.map(|u| *u.as_uuid()))
            .bind(task_id)
            .bind(conversation_id.map(|c| *c.as_uuid()))
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn get_by_id(&self, id: WorktreeId) -> Result<Worktree, AppError> {
        let query = format!("SELECT {WORKTREE_COLS} FROM worktrees WHERE id = $1");
        let row = sqlx::query_as::<_, WorktreeRow>(&query)
            .bind(id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?
            .ok_or_else(|| AppError::NotFound("worktree".into()))?;

        Ok(row.into())
    }

    async fn list_by_repo(&self, repo_id: WorkspaceRepoId) -> Result<Vec<Worktree>, AppError> {
        let query = format!(
            "SELECT {WORKTREE_COLS} FROM worktrees WHERE repo_id = $1 \
             ORDER BY created_at DESC"
        );
        let rows = sqlx::query_as::<_, WorktreeRow>(&query)
            .bind(repo_id.as_uuid())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_stale_candidates(
        &self,
        older_than: DateTime<Utc>,
    ) -> Result<Vec<Worktree>, AppError> {
        let query = format!(
            "SELECT {WORKTREE_COLS} FROM worktrees \
             WHERE state = 'active' AND last_active_at < $1 \
             ORDER BY last_active_at ASC"
        );
        let rows = sqlx::query_as::<_, WorktreeRow>(&query)
            .bind(older_than)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn mark_stale(&self, id: WorktreeId) -> Result<(), AppError> {
        let result = sqlx::query("UPDATE worktrees SET state = 'stale' WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("worktree".into()));
        }

        Ok(())
    }

    async fn mark_removing(&self, id: WorktreeId) -> Result<(), AppError> {
        let result = sqlx::query("UPDATE worktrees SET state = 'removing' WHERE id = $1")
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
