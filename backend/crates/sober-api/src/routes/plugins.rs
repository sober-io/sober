//! Unified plugin management routes.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::ApiResponse;

use crate::services::plugin::{
    AuditLogEntry, ImportResult, PluginInfo, ReloadResult, SkillInfo, ToolInfo,
};
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
        .route("/skills", get(list_skills))
        .route("/skills/reload", post(reload_skills))
        .route("/tools", get(list_tools))
}

#[derive(serde::Deserialize)]
struct ListPluginsParams {
    kind: Option<String>,
    status: Option<String>,
    workspace_id: Option<String>,
}

async fn list_plugins(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Query(params): Query<ListPluginsParams>,
) -> Result<ApiResponse<Vec<PluginInfo>>, AppError> {
    let plugins = state
        .plugin
        .list(params.kind, params.status, params.workspace_id)
        .await?;
    Ok(ApiResponse::new(plugins))
}

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

async fn install_plugin(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Json(body): Json<InstallPluginRequest>,
) -> Result<ApiResponse<Option<PluginInfo>>, AppError> {
    let plugin = state
        .plugin
        .install(
            body.name,
            body.kind,
            body.config,
            body.description,
            body.version,
        )
        .await?;
    Ok(ApiResponse::new(plugin))
}

#[derive(serde::Deserialize)]
struct ImportPluginsRequest {
    #[serde(alias = "mcpServers")]
    mcp_servers: serde_json::Value,
}

async fn import_plugins(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Json(body): Json<ImportPluginsRequest>,
) -> Result<ApiResponse<ImportResult>, AppError> {
    let result = state.plugin.import(body.mcp_servers).await?;
    Ok(ApiResponse::new(result))
}

async fn reload_plugins(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
) -> Result<ApiResponse<ReloadResult>, AppError> {
    let result = state.plugin.reload().await?;
    Ok(ApiResponse::new(result))
}

async fn get_plugin(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<PluginInfo>, AppError> {
    let plugin = state.plugin.get(id).await?;
    Ok(ApiResponse::new(plugin))
}

#[derive(serde::Deserialize)]
struct UpdatePluginRequest {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    config: Option<serde_json::Value>,
    #[serde(default)]
    scope: Option<String>,
}

async fn update_plugin(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<UpdatePluginRequest>,
) -> Result<ApiResponse<PluginInfo>, AppError> {
    let plugin = state
        .plugin
        .update(id, body.enabled, body.config, body.scope)
        .await?;
    Ok(ApiResponse::new(plugin))
}

async fn uninstall_plugin(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state.plugin.uninstall(id).await?;
    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}

#[derive(serde::Deserialize)]
struct AuditLogParams {
    #[serde(default = "default_audit_limit")]
    limit: i64,
}

fn default_audit_limit() -> i64 {
    50
}

async fn list_audit_logs(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Query(params): Query<AuditLogParams>,
) -> Result<ApiResponse<Vec<AuditLogEntry>>, AppError> {
    let entries = state.plugin.list_audit_logs(id, params.limit).await?;
    Ok(ApiResponse::new(entries))
}

#[derive(serde::Deserialize)]
struct ListSkillsParams {
    conversation_id: Option<String>,
}

async fn list_skills(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Query(params): Query<ListSkillsParams>,
) -> Result<ApiResponse<Vec<SkillInfo>>, AppError> {
    let skills = state
        .plugin
        .list_skills(auth_user.user_id, params.conversation_id)
        .await?;
    Ok(ApiResponse::new(skills))
}

async fn reload_skills(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Query(params): Query<ListSkillsParams>,
) -> Result<ApiResponse<Vec<SkillInfo>>, AppError> {
    let skills = state
        .plugin
        .reload_skills(auth_user.user_id, params.conversation_id)
        .await?;
    Ok(ApiResponse::new(skills))
}

async fn list_tools(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
) -> Result<ApiResponse<Vec<ToolInfo>>, AppError> {
    let tools = state.plugin.list_tools().await?;
    Ok(ApiResponse::new(tools))
}
