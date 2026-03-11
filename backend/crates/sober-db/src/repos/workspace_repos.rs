//! PostgreSQL implementation of [`WorkspaceRepoRepo`].

use sober_core::error::AppError;
use sober_core::types::{RegisterRepo, UserId, WorkspaceId, WorkspaceRepoEntry, WorkspaceRepoId};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::WorkspaceRepoRow;

/// PostgreSQL-backed workspace git repository registry.
pub struct PgWorkspaceRepoRepo {
    pool: PgPool,
}

impl PgWorkspaceRepoRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::WorkspaceRepoRepo for PgWorkspaceRepoRepo {
    async fn register(
        &self,
        workspace_id: WorkspaceId,
        input: RegisterRepo,
    ) -> Result<WorkspaceRepoEntry, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, WorkspaceRepoRow>(
            "INSERT INTO workspace_repos (id, workspace_id, name, path, is_linked, remote_url, default_branch) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING id, workspace_id, name, path, is_linked, remote_url, default_branch, created_at",
        )
        .bind(id)
        .bind(workspace_id.as_uuid())
        .bind(&input.name)
        .bind(&input.path)
        .bind(input.is_linked)
        .bind(&input.remote_url)
        .bind(&input.default_branch)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => {
                AppError::Conflict("repo path already registered in this workspace".into())
            }
            other => AppError::Internal(other.into()),
        })?;

        Ok(row.into())
    }

    async fn list_by_workspace(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<WorkspaceRepoEntry>, AppError> {
        let rows = sqlx::query_as::<_, WorkspaceRepoRow>(
            "SELECT id, workspace_id, name, path, is_linked, remote_url, default_branch, created_at \
             FROM workspace_repos WHERE workspace_id = $1 \
             ORDER BY name ASC",
        )
        .bind(workspace_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn find_by_linked_path(
        &self,
        path: &str,
        user_id: UserId,
    ) -> Result<Option<WorkspaceRepoEntry>, AppError> {
        let row = sqlx::query_as::<_, WorkspaceRepoRow>(
            "SELECT wr.id, wr.workspace_id, wr.name, wr.path, wr.is_linked, \
                    wr.remote_url, wr.default_branch, wr.created_at \
             FROM workspace_repos wr \
             JOIN workspaces w ON w.id = wr.workspace_id \
             WHERE wr.path = $1 AND w.user_id = $2 \
               AND wr.is_linked = true AND w.state = 'active'",
        )
        .bind(path)
        .bind(user_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.map(Into::into))
    }

    async fn delete(&self, id: WorkspaceRepoId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM workspace_repos WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("workspace_repo".into()));
        }

        Ok(())
    }
}
