//! PostgreSQL implementation of [`WorkspaceRepo`].

use sober_core::error::AppError;
use sober_core::types::{UserId, Workspace, WorkspaceId, WorkspaceSettings};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::{WorkspaceRow, WorkspaceSettingsRow};

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
        description: Option<&str>,
        root_path: &str,
    ) -> Result<Workspace, AppError> {
        let id = Uuid::now_v7();
        let uid = user_id.as_uuid();
        let row = sqlx::query_as::<_, WorkspaceRow>(
            "INSERT INTO workspaces (id, user_id, name, description, root_path, created_by) \
             VALUES ($1, $2, $3, $4, $5, $2) \
             RETURNING id, user_id, name, description, root_path, state, \
                       created_by, archived_at, deleted_at, created_at, updated_at",
        )
        .bind(id)
        .bind(uid)
        .bind(name)
        .bind(description)
        .bind(root_path)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn get_by_id(&self, id: WorkspaceId) -> Result<Workspace, AppError> {
        let row = sqlx::query_as::<_, WorkspaceRow>(
            "SELECT id, user_id, name, description, root_path, state, \
                    created_by, archived_at, deleted_at, created_at, updated_at \
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
            "SELECT id, user_id, name, description, root_path, state, \
                    created_by, archived_at, deleted_at, created_at, updated_at \
             FROM workspaces WHERE user_id = $1 AND state != 'deleted' \
             ORDER BY updated_at DESC",
        )
        .bind(user_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn archive(&self, id: WorkspaceId) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE workspaces SET state = 'archived', archived_at = now(), updated_at = now() \
             WHERE id = $1",
        )
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
        let result = sqlx::query(
            "UPDATE workspaces SET state = 'active', archived_at = NULL, updated_at = now() \
             WHERE id = $1",
        )
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
        let result = sqlx::query(
            "UPDATE workspaces SET state = 'deleted', deleted_at = now(), updated_at = now() \
             WHERE id = $1",
        )
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("workspace".into()));
        }

        Ok(())
    }

    async fn provision(
        &self,
        user_id: UserId,
        name: &str,
        root_path: &str,
    ) -> Result<(Workspace, WorkspaceSettings), AppError> {
        let id = Uuid::now_v7();
        let uid = user_id.as_uuid();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let ws_row = sqlx::query_as::<_, WorkspaceRow>(
            "INSERT INTO workspaces (id, user_id, name, root_path, created_by) \
             VALUES ($1, $2, $3, $4, $2) \
             RETURNING id, user_id, name, description, root_path, state, \
                       created_by, archived_at, deleted_at, created_at, updated_at",
        )
        .bind(id)
        .bind(uid)
        .bind(name)
        .bind(root_path)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        let settings_row = sqlx::query_as::<_, WorkspaceSettingsRow>(
            "INSERT INTO workspace_settings (workspace_id) \
             VALUES ($1) \
             RETURNING workspace_id, permission_mode, auto_snapshot, max_snapshots, \
                       sandbox_profile, sandbox_net_mode, sandbox_allowed_domains, \
                       sandbox_max_execution_seconds, sandbox_allow_spawn, \
                       disabled_tools, disabled_plugins, \
                       created_at, updated_at",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok((ws_row.into(), settings_row.into()))
    }
}
