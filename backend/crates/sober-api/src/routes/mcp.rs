//! MCP server configuration CRUD route handlers.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, CreateMcpServer, McpServerId, McpServerRepo, UpdateMcpServer,
};
use sober_db::PgMcpServerRepo;

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
    let repo = PgMcpServerRepo::new(state.db.clone());
    let servers = repo.list_by_user(auth_user.user_id).await?;

    let items: Vec<serde_json::Value> = servers
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id.to_string(),
                "name": s.name,
                "command": s.command,
                "args": s.args,
                "env": s.env,
                "enabled": s.enabled,
                "created_at": s.created_at.to_rfc3339(),
                "updated_at": s.updated_at.to_rfc3339(),
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
    let repo = PgMcpServerRepo::new(state.db.clone());
    let input = CreateMcpServer {
        user_id: auth_user.user_id,
        name: body.name,
        command: body.command,
        args: body.args,
        env: body.env,
    };
    let server = repo.create(input).await?;

    Ok(ApiResponse::new(serde_json::json!({
        "id": server.id.to_string(),
        "name": server.name,
        "command": server.command,
        "args": server.args,
        "env": server.env,
        "enabled": server.enabled,
        "created_at": server.created_at.to_rfc3339(),
        "updated_at": server.updated_at.to_rfc3339(),
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
    let repo = PgMcpServerRepo::new(state.db.clone());
    let server_id = McpServerId::from_uuid(id);

    // Verify ownership by checking the server belongs to this user.
    let servers = repo.list_by_user(auth_user.user_id).await?;
    if !servers.iter().any(|s| s.id == server_id) {
        return Err(AppError::NotFound("mcp_server".into()));
    }

    let input = UpdateMcpServer {
        name: body.name,
        command: body.command,
        args: body.args,
        env: body.env,
        enabled: body.enabled,
    };
    let server = repo.update(server_id, input).await?;

    Ok(ApiResponse::new(serde_json::json!({
        "id": server.id.to_string(),
        "name": server.name,
        "command": server.command,
        "args": server.args,
        "env": server.env,
        "enabled": server.enabled,
        "created_at": server.created_at.to_rfc3339(),
        "updated_at": server.updated_at.to_rfc3339(),
    })))
}

/// `DELETE /api/v1/mcp/servers/:id` — remove an MCP server configuration.
async fn delete_server(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let repo = PgMcpServerRepo::new(state.db.clone());
    let server_id = McpServerId::from_uuid(id);

    // Verify ownership.
    let servers = repo.list_by_user(auth_user.user_id).await?;
    if !servers.iter().any(|s| s.id == server_id) {
        return Err(AppError::NotFound("mcp_server".into()));
    }

    repo.delete(server_id).await?;

    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}
