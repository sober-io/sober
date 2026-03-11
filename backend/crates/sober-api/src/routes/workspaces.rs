//! Workspace settings route handlers.

use std::path::Path;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::routing::put;
use axum::{Json, Router};
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{ApiResponse, WorkspaceId, WorkspaceRepo};
use sober_core::{PermissionMode, WorkspaceConfig, WorkspaceShellConfig};
use sober_db::PgWorkspaceRepo;
use tracing::info;

use crate::state::AppState;

/// Returns the workspace routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/workspaces/{id}/settings", put(update_workspace_settings))
}

/// Request body for `PUT /workspaces/:id/settings`.
#[derive(Debug, serde::Deserialize)]
struct UpdateWorkspaceSettings {
    /// Permission mode: "interactive", "policy_based", or "autonomous".
    permission_mode: Option<PermissionMode>,
    /// Whether to auto-snapshot before dangerous commands.
    auto_snapshot: Option<bool>,
}

/// Response body for workspace settings.
#[derive(Debug, serde::Serialize)]
struct WorkspaceSettingsResponse {
    permission_mode: PermissionMode,
    auto_snapshot: bool,
}

/// `PUT /api/v1/workspaces/:id/settings` — update workspace shell settings.
async fn update_workspace_settings(
    State(state): State<Arc<AppState>>,
    AxumPath(workspace_id): AxumPath<uuid::Uuid>,
    auth_user: AuthUser,
    Json(body): Json<UpdateWorkspaceSettings>,
) -> Result<ApiResponse<WorkspaceSettingsResponse>, AppError> {
    let repo = PgWorkspaceRepo::new(state.db.clone());
    let ws_id = WorkspaceId::from_uuid(workspace_id);
    let workspace = repo.get_by_id(ws_id).await?;

    // Verify ownership.
    if workspace.user_id != auth_user.user_id {
        return Err(AppError::NotFound("workspace".into()));
    }

    let config_path = Path::new(&workspace.root_path)
        .join(".sober")
        .join("config.toml");

    // Read existing config (or use defaults if file doesn't exist).
    let mut config = if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        WorkspaceConfig::from_toml(&content)
            .map_err(|e| AppError::Validation(format!("invalid config.toml: {e}")))?
    } else {
        WorkspaceConfig::default()
    };

    // Update shell settings.
    {
        let shell = config.shell.get_or_insert_with(|| WorkspaceShellConfig {
            permission_mode: PermissionMode::default(),
            auto_snapshot: None,
            max_snapshots: None,
            rules: None,
        });

        if let Some(mode) = body.permission_mode {
            shell.permission_mode = mode;
        }
        if let Some(snap) = body.auto_snapshot {
            shell.auto_snapshot = Some(snap);
        }
    }

    // Write back.
    let toml_str = toml::to_string_pretty(&config).map_err(|e| AppError::Internal(e.into()))?;

    // Ensure .sober directory exists.
    if let Some(parent) = config_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    }

    tokio::fs::write(&config_path, &toml_str)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    // Extract final values for the response.
    let shell = config
        .shell
        .as_ref()
        .expect("shell section was just inserted");

    info!(
        workspace_id = %workspace_id,
        permission_mode = ?shell.permission_mode,
        "updated workspace shell settings"
    );

    let response = WorkspaceSettingsResponse {
        permission_mode: shell.permission_mode,
        auto_snapshot: shell.auto_snapshot.unwrap_or(true),
    };

    Ok(ApiResponse::new(response))
}
