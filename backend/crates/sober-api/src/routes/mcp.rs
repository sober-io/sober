//! MCP server configuration CRUD route handlers.
//!
//! These routes provide backward-compatible REST endpoints for MCP server
//! management. Under the hood they now use the unified `PluginRepo` with
//! `PluginKind::Mcp`. A full replacement with unified plugin routes is
//! planned for Task 6.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, CreatePlugin, PluginFilter, PluginId, PluginKind, PluginOrigin, PluginRepo,
    PluginScope, PluginStatus,
};
use sober_db::PgPluginRepo;

use crate::state::AppState;

/// Returns the MCP server config routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/mcp/servers", get(list_servers))
        .route("/mcp/servers", post(create_server))
        .route("/mcp/servers/{id}", patch(update_server))
        .route("/mcp/servers/{id}", delete(delete_server))
}

/// `GET /api/v1/mcp/servers` — list MCP server configs for the user.
async fn list_servers(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let repo = PgPluginRepo::new(state.db.clone());
    let filter = PluginFilter {
        kind: Some(PluginKind::Mcp),
        owner_id: Some(auth_user.user_id),
        ..Default::default()
    };
    let plugins = repo.list(filter).await?;

    let items: Vec<serde_json::Value> = plugins
        .into_iter()
        .map(|p| {
            let command = p
                .config
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let args = p
                .config
                .get("args")
                .cloned()
                .unwrap_or(serde_json::json!([]));
            let env = p
                .config
                .get("env")
                .cloned()
                .unwrap_or(serde_json::json!({}));
            let enabled = p.status == PluginStatus::Enabled;

            serde_json::json!({
                "id": p.id.to_string(),
                "name": p.name,
                "command": command,
                "args": args,
                "env": env,
                "enabled": enabled,
                "created_at": p.installed_at.to_rfc3339(),
                "updated_at": p.updated_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(ApiResponse::new(serde_json::json!(items)))
}

/// Request body for `POST /mcp/servers`.
#[derive(serde::Deserialize)]
struct CreateServerRequest {
    name: String,
    command: String,
    #[serde(default = "default_json_array")]
    args: serde_json::Value,
    #[serde(default = "default_json_object")]
    env: serde_json::Value,
}

fn default_json_array() -> serde_json::Value {
    serde_json::Value::Array(vec![])
}

fn default_json_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// `POST /api/v1/mcp/servers` — add an MCP server configuration.
async fn create_server(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Json(body): Json<CreateServerRequest>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let repo = PgPluginRepo::new(state.db.clone());
    let config = serde_json::json!({
        "command": body.command,
        "args": body.args,
        "env": body.env,
    });
    let input = CreatePlugin {
        name: body.name,
        kind: PluginKind::Mcp,
        version: None,
        description: None,
        origin: PluginOrigin::User,
        scope: PluginScope::User,
        owner_id: Some(auth_user.user_id),
        workspace_id: None,
        status: PluginStatus::Enabled,
        config,
        installed_by: Some(auth_user.user_id),
    };
    let plugin = repo.create(input).await?;

    let command = plugin
        .config
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let args = plugin
        .config
        .get("args")
        .cloned()
        .unwrap_or(serde_json::json!([]));
    let env = plugin
        .config
        .get("env")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    Ok(ApiResponse::new(serde_json::json!({
        "id": plugin.id.to_string(),
        "name": plugin.name,
        "command": command,
        "args": args,
        "env": env,
        "enabled": plugin.status == PluginStatus::Enabled,
        "created_at": plugin.installed_at.to_rfc3339(),
        "updated_at": plugin.updated_at.to_rfc3339(),
    })))
}

/// Request body for `PATCH /mcp/servers/:id`.
#[derive(serde::Deserialize)]
struct UpdateServerRequest {
    name: Option<String>,
    command: Option<String>,
    args: Option<serde_json::Value>,
    env: Option<serde_json::Value>,
    enabled: Option<bool>,
}

/// `PATCH /api/v1/mcp/servers/:id` — update an MCP server configuration.
async fn update_server(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<UpdateServerRequest>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let repo = PgPluginRepo::new(state.db.clone());
    let plugin_id = PluginId::from_uuid(id);

    // Verify the plugin exists and belongs to this user.
    let plugin = repo.get_by_id(plugin_id).await?;
    if plugin.owner_id != Some(auth_user.user_id) {
        return Err(AppError::NotFound("mcp_server".into()));
    }

    // Update enabled/disabled status if requested.
    if let Some(enabled) = body.enabled {
        let status = if enabled {
            PluginStatus::Enabled
        } else {
            PluginStatus::Disabled
        };
        repo.update_status(plugin_id, status).await?;
    }

    // Build updated config if any config fields changed.
    if body.name.is_some() || body.command.is_some() || body.args.is_some() || body.env.is_some() {
        let mut config = plugin.config.clone();
        if let Some(command) = &body.command {
            config["command"] = serde_json::Value::String(command.clone());
        }
        if let Some(args) = &body.args {
            config["args"] = args.clone();
        }
        if let Some(env) = &body.env {
            config["env"] = env.clone();
        }
        repo.update_config(plugin_id, config).await?;
    }

    // TODO(#019/task-6): name updates require a dedicated `update_name` method
    // on PluginRepo. For now, name changes are silently ignored.

    // Re-fetch to return the updated state.
    let updated = repo.get_by_id(plugin_id).await?;
    let command = updated
        .config
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let args = updated
        .config
        .get("args")
        .cloned()
        .unwrap_or(serde_json::json!([]));
    let env = updated
        .config
        .get("env")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    Ok(ApiResponse::new(serde_json::json!({
        "id": updated.id.to_string(),
        "name": updated.name,
        "command": command,
        "args": args,
        "env": env,
        "enabled": updated.status == PluginStatus::Enabled,
        "created_at": updated.installed_at.to_rfc3339(),
        "updated_at": updated.updated_at.to_rfc3339(),
    })))
}

/// `DELETE /api/v1/mcp/servers/:id` — remove an MCP server configuration.
async fn delete_server(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let repo = PgPluginRepo::new(state.db.clone());
    let plugin_id = PluginId::from_uuid(id);

    // Verify ownership.
    let plugin = repo.get_by_id(plugin_id).await?;
    if plugin.owner_id != Some(auth_user.user_id) {
        return Err(AppError::NotFound("mcp_server".into()));
    }

    repo.delete(plugin_id).await?;

    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}
