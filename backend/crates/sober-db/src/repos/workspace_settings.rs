//! PostgreSQL implementation of [`WorkspaceSettingsRepo`].

use sober_core::error::AppError;
use sober_core::types::{WorkspaceId, WorkspaceSettings};
use sqlx::PgPool;

use crate::rows::WorkspaceSettingsRow;

/// PostgreSQL-backed workspace settings repository.
pub struct PgWorkspaceSettingsRepo {
    pool: PgPool,
}

impl PgWorkspaceSettingsRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::WorkspaceSettingsRepo for PgWorkspaceSettingsRepo {
    async fn get_by_workspace(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<WorkspaceSettings, AppError> {
        let row = sqlx::query_as::<_, WorkspaceSettingsRow>(
            "SELECT workspace_id, permission_mode, auto_snapshot, max_snapshots, \
                    sandbox_profile, sandbox_net_mode, sandbox_allowed_domains, \
                    sandbox_max_execution_seconds, sandbox_allow_spawn, \
                    disabled_tools, disabled_plugins, \
                    created_at, updated_at \
             FROM workspace_settings WHERE workspace_id = $1",
        )
        .bind(workspace_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("workspace_settings".into()))?;

        Ok(row.into())
    }

    async fn upsert(&self, settings: &WorkspaceSettings) -> Result<WorkspaceSettings, AppError> {
        let disabled_plugin_uuids: Vec<uuid::Uuid> = settings
            .disabled_plugins
            .iter()
            .map(|id| *id.as_uuid())
            .collect();

        let row = sqlx::query_as::<_, WorkspaceSettingsRow>(
            "INSERT INTO workspace_settings \
                (workspace_id, permission_mode, auto_snapshot, max_snapshots, \
                 sandbox_profile, sandbox_net_mode, sandbox_allowed_domains, \
                 sandbox_max_execution_seconds, sandbox_allow_spawn, \
                 disabled_tools, disabled_plugins) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             ON CONFLICT (workspace_id) DO UPDATE SET \
                permission_mode = EXCLUDED.permission_mode, \
                auto_snapshot = EXCLUDED.auto_snapshot, \
                max_snapshots = EXCLUDED.max_snapshots, \
                sandbox_profile = EXCLUDED.sandbox_profile, \
                sandbox_net_mode = EXCLUDED.sandbox_net_mode, \
                sandbox_allowed_domains = EXCLUDED.sandbox_allowed_domains, \
                sandbox_max_execution_seconds = EXCLUDED.sandbox_max_execution_seconds, \
                sandbox_allow_spawn = EXCLUDED.sandbox_allow_spawn, \
                disabled_tools = EXCLUDED.disabled_tools, \
                disabled_plugins = EXCLUDED.disabled_plugins, \
                updated_at = now() \
             RETURNING workspace_id, permission_mode, auto_snapshot, max_snapshots, \
                       sandbox_profile, sandbox_net_mode, sandbox_allowed_domains, \
                       sandbox_max_execution_seconds, sandbox_allow_spawn, \
                       disabled_tools, disabled_plugins, \
                       created_at, updated_at",
        )
        .bind(settings.workspace_id.as_uuid())
        .bind(settings.permission_mode)
        .bind(settings.auto_snapshot)
        .bind(settings.max_snapshots)
        .bind(&settings.sandbox_profile)
        .bind(settings.sandbox_net_mode)
        .bind(&settings.sandbox_allowed_domains)
        .bind(settings.sandbox_max_execution_seconds)
        .bind(settings.sandbox_allow_spawn)
        .bind(&settings.disabled_tools)
        .bind(&disabled_plugin_uuids)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }
}
