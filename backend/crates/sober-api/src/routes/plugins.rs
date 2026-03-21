//! Unified plugin management routes.
//!
//! These routes proxy plugin operations to the agent gRPC service. They replace
//! the old MCP server and skills routes with a single `/plugins` namespace.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{ApiResponse, PluginId, PluginRepo};
use sober_db::PgPluginRepo;

use crate::proto;
use crate::state::AppState;

/// Returns the plugin management routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/plugins", get(list_plugins))
        .route("/plugins", post(install_plugin))
        .route("/plugins/import", post(import_plugins))
        .route("/plugins/reload", post(reload_plugins))
        .route("/plugins/{id}", get(get_plugin))
        .route("/plugins/{id}", patch(update_plugin))
        .route("/plugins/{id}", delete(uninstall_plugin))
        .route("/plugins/{id}/audit", get(list_audit_logs))
}

/// Query parameters for `GET /api/v1/plugins`.
#[derive(serde::Deserialize)]
struct ListPluginsParams {
    kind: Option<String>,
    status: Option<String>,
}

/// `GET /api/v1/plugins` — list plugins with optional filters.
///
/// Proxies to the agent's `ListPlugins` gRPC RPC. Accepts optional `kind` and
/// `status` query parameters for filtering.
async fn list_plugins(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Query(params): Query<ListPluginsParams>,
) -> Result<ApiResponse<Vec<serde_json::Value>>, AppError> {
    let mut client = state.agent_client.clone();

    let response = client
        .list_plugins(proto::ListPluginsRequest {
            kind: params.kind,
            status: params.status,
        })
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let plugins: Vec<serde_json::Value> = response
        .into_inner()
        .plugins
        .into_iter()
        .map(plugin_info_to_json)
        .collect();

    Ok(ApiResponse::new(plugins))
}

/// Request body for `POST /api/v1/plugins`.
#[derive(serde::Deserialize)]
struct InstallPluginRequest {
    name: String,
    kind: String,
    config: serde_json::Value,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

/// `POST /api/v1/plugins` — install a new plugin.
///
/// Proxies to the agent's `InstallPlugin` gRPC RPC. The config field is a
/// JSON object specific to the plugin kind (e.g., MCP server config with
/// `command`, `args`, `env`).
async fn install_plugin(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Json(body): Json<InstallPluginRequest>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let mut client = state.agent_client.clone();

    let config_str =
        serde_json::to_string(&body.config).map_err(|e| AppError::Internal(e.into()))?;

    let response = client
        .install_plugin(proto::InstallPluginRequest {
            name: body.name,
            kind: body.kind,
            config: config_str,
            description: body.description,
            version: body.version,
        })
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let inner = response.into_inner();
    let plugin_json = inner
        .plugin
        .map(plugin_info_to_json)
        .unwrap_or(serde_json::json!(null));

    Ok(ApiResponse::new(plugin_json))
}

/// Request body for `POST /api/v1/plugins/import`.
#[derive(serde::Deserialize)]
struct ImportPluginsRequest {
    /// The `mcpServers` configuration object from `.mcp.json` format.
    #[serde(alias = "mcpServers")]
    mcp_servers: serde_json::Value,
}

/// `POST /api/v1/plugins/import` — batch import plugins from `.mcp.json` config.
///
/// Proxies to the agent's `ImportPlugins` gRPC RPC. Accepts the standard
/// `mcpServers` JSON object and creates a plugin entry for each server.
async fn import_plugins(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Json(body): Json<ImportPluginsRequest>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let mut client = state.agent_client.clone();

    let mcp_servers_json =
        serde_json::to_string(&body.mcp_servers).map_err(|e| AppError::Internal(e.into()))?;

    let response = client
        .import_plugins(proto::ImportPluginsRequest { mcp_servers_json })
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let inner = response.into_inner();
    let plugins: Vec<serde_json::Value> =
        inner.plugins.into_iter().map(plugin_info_to_json).collect();

    Ok(ApiResponse::new(serde_json::json!({
        "imported_count": inner.imported_count,
        "plugins": plugins,
    })))
}

/// `POST /api/v1/plugins/reload` — re-scan and reload all plugins.
///
/// Proxies to the agent's `ReloadPlugins` gRPC RPC. Forces the agent to
/// re-read plugin configs and restart any managed processes.
async fn reload_plugins(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let mut client = state.agent_client.clone();

    let response = client
        .reload_plugins(proto::ReloadPluginsRequest {})
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let inner = response.into_inner();

    Ok(ApiResponse::new(serde_json::json!({
        "active_count": inner.active_count,
    })))
}

/// `GET /api/v1/plugins/:id` — get a single plugin by ID.
///
/// Proxies to the agent's `ListPlugins` gRPC RPC, filtering by the specific
/// plugin ID in the response. Returns 404 if no plugin matches.
async fn get_plugin(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let mut client = state.agent_client.clone();

    let response = client
        .list_plugins(proto::ListPluginsRequest {
            kind: None,
            status: None,
        })
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let id_str = id.to_string();
    let plugin = response
        .into_inner()
        .plugins
        .into_iter()
        .find(|p| p.id == id_str)
        .ok_or_else(|| AppError::NotFound("plugin".into()))?;

    Ok(ApiResponse::new(plugin_info_to_json(plugin)))
}

/// Request body for `PATCH /api/v1/plugins/:id`.
#[derive(serde::Deserialize)]
struct UpdatePluginRequest {
    /// Enable or disable the plugin.
    #[serde(default)]
    enabled: Option<bool>,
    /// Updated configuration (merged with existing).
    #[serde(default)]
    config: Option<serde_json::Value>,
}

/// `PATCH /api/v1/plugins/:id` — update a plugin (enable/disable, change config).
///
/// Proxies to the agent's `EnablePlugin`/`DisablePlugin` gRPC RPCs for status
/// changes, and uses `PgPluginRepo::update_config` directly for config updates.
async fn update_plugin(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<UpdatePluginRequest>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let mut client = state.agent_client.clone();
    let plugin_id = PluginId::from_uuid(id);
    let id_str = id.to_string();

    // Handle enable/disable via gRPC.
    if let Some(enabled) = body.enabled {
        if enabled {
            client
                .enable_plugin(proto::EnablePluginRequest {
                    plugin_id: id_str.clone(),
                })
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        } else {
            client
                .disable_plugin(proto::DisablePluginRequest {
                    plugin_id: id_str.clone(),
                })
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
    }

    // Handle config update via DB repo directly.
    if let Some(config) = body.config {
        let repo = PgPluginRepo::new(state.db.clone());
        repo.update_config(plugin_id, config).await?;
    }

    // Re-fetch via gRPC to return consistent state.
    let response = client
        .list_plugins(proto::ListPluginsRequest {
            kind: None,
            status: None,
        })
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let plugin = response
        .into_inner()
        .plugins
        .into_iter()
        .find(|p| p.id == id_str)
        .ok_or_else(|| AppError::NotFound("plugin".into()))?;

    Ok(ApiResponse::new(plugin_info_to_json(plugin)))
}

/// `DELETE /api/v1/plugins/:id` — uninstall a plugin.
///
/// Proxies to the agent's `UninstallPlugin` gRPC RPC. The agent handles
/// process shutdown and database cleanup.
async fn uninstall_plugin(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let mut client = state.agent_client.clone();

    client
        .uninstall_plugin(proto::UninstallPluginRequest {
            plugin_id: id.to_string(),
        })
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}

/// Query parameters for `GET /api/v1/plugins/:id/audit`.
#[derive(serde::Deserialize)]
struct AuditLogParams {
    #[serde(default = "default_audit_limit")]
    limit: i64,
}

fn default_audit_limit() -> i64 {
    50
}

/// `GET /api/v1/plugins/:id/audit` — list audit log entries for a plugin.
///
/// Queries `PgPluginRepo::list_audit_logs` directly since there is no gRPC
/// RPC for this yet. Returns audit entries newest first.
async fn list_audit_logs(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Query(params): Query<AuditLogParams>,
) -> Result<ApiResponse<Vec<serde_json::Value>>, AppError> {
    let repo = PgPluginRepo::new(state.db.clone());
    let plugin_id = PluginId::from_uuid(id);

    let logs = repo.list_audit_logs(plugin_id, params.limit).await?;

    let entries: Vec<serde_json::Value> = logs
        .into_iter()
        .map(|log| {
            serde_json::json!({
                "id": log.id.to_string(),
                "plugin_id": log.plugin_id.map(|id| id.to_string()),
                "plugin_name": log.plugin_name,
                "kind": log.kind,
                "origin": log.origin,
                "stages": log.stages,
                "verdict": log.verdict,
                "rejection_reason": log.rejection_reason,
                "audited_at": log.audited_at.to_rfc3339(),
                "audited_by": log.audited_by.map(|id| id.to_string()),
            })
        })
        .collect();

    Ok(ApiResponse::new(entries))
}

/// Converts a proto `PluginInfo` message to a JSON value for API responses.
fn plugin_info_to_json(info: proto::PluginInfo) -> serde_json::Value {
    // Parse config string back to JSON value for the response.
    let config: serde_json::Value =
        serde_json::from_str(&info.config).unwrap_or(serde_json::json!({}));

    serde_json::json!({
        "id": info.id,
        "name": info.name,
        "kind": info.kind,
        "version": info.version,
        "description": info.description,
        "status": info.status,
        "config": config,
        "installed_at": info.installed_at,
    })
}
