//! Conversation CRUD route handlers.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use sober_auth::AuthUser;
use sober_core::PermissionMode;
use sober_core::error::AppError;
use sober_core::types::{ApiResponse, ConversationId, ConversationRepo, MessageRepo, WorkspaceId};
use sober_db::{PgConversationRepo, PgMessageRepo};

use crate::state::AppState;

/// Returns the conversation routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/conversations", get(list_conversations))
        .route("/conversations", post(create_conversation))
        .route("/conversations/{id}", get(get_conversation))
        .route("/conversations/{id}", patch(update_conversation))
        .route("/conversations/{id}", delete(delete_conversation))
}

/// Serialize a [`PermissionMode`] to its API string.
fn permission_mode_str(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Interactive => "interactive",
        PermissionMode::PolicyBased => "policy_based",
        PermissionMode::Autonomous => "autonomous",
    }
}

/// `GET /api/v1/conversations` — list conversations for the authenticated user.
async fn list_conversations(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let repo = PgConversationRepo::new(state.db.clone());
    let conversations = repo.list_by_user(auth_user.user_id).await?;

    let items: Vec<serde_json::Value> = conversations
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id.to_string(),
                "title": c.title,
                "workspace_id": c.workspace_id.map(|w| w.to_string()),
                "permission_mode": permission_mode_str(c.permission_mode),
                "created_at": c.created_at.to_rfc3339(),
                "updated_at": c.updated_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(ApiResponse::new(serde_json::json!(items)))
}

/// Request body for `POST /conversations`.
#[derive(serde::Deserialize)]
struct CreateConversationRequest {
    title: Option<String>,
    workspace_id: Option<String>,
}

/// `POST /api/v1/conversations` — create a new conversation.
async fn create_conversation(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Json(body): Json<CreateConversationRequest>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let repo = PgConversationRepo::new(state.db.clone());
    let workspace_id = body
        .workspace_id
        .as_deref()
        .map(|s| {
            s.parse::<uuid::Uuid>()
                .map(WorkspaceId::from_uuid)
                .map_err(|_| AppError::Validation("invalid workspace_id".into()))
        })
        .transpose()?;
    let conversation = repo
        .create(auth_user.user_id, body.title.as_deref(), workspace_id)
        .await?;

    Ok(ApiResponse::new(serde_json::json!({
        "id": conversation.id.to_string(),
        "title": conversation.title,
        "workspace_id": conversation.workspace_id.map(|w| w.to_string()),
        "permission_mode": permission_mode_str(conversation.permission_mode),
        "created_at": conversation.created_at.to_rfc3339(),
        "updated_at": conversation.updated_at.to_rfc3339(),
    })))
}

/// `GET /api/v1/conversations/:id` — get a conversation with its messages.
async fn get_conversation(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let conv_repo = PgConversationRepo::new(state.db.clone());
    let msg_repo = PgMessageRepo::new(state.db.clone());

    let conversation_id = ConversationId::from_uuid(id);
    let conversation = conv_repo.get_by_id(conversation_id).await?;

    // Verify ownership.
    if conversation.user_id != auth_user.user_id {
        return Err(AppError::NotFound("conversation".into()));
    }

    let messages = msg_repo.list_by_conversation(conversation_id, 1000).await?;
    let messages: Vec<serde_json::Value> = messages
        .into_iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id.to_string(),
                "role": format!("{:?}", m.role),
                "content": m.content,
                "tool_calls": m.tool_calls,
                "tool_result": m.tool_result,
                "token_count": m.token_count,
                "created_at": m.created_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(ApiResponse::new(serde_json::json!({
        "id": conversation.id.to_string(),
        "title": conversation.title,
        "workspace_id": conversation.workspace_id.map(|w| w.to_string()),
        "permission_mode": permission_mode_str(conversation.permission_mode),
        "created_at": conversation.created_at.to_rfc3339(),
        "updated_at": conversation.updated_at.to_rfc3339(),
        "messages": messages,
    })))
}

/// Request body for `PATCH /conversations/:id`.
#[derive(serde::Deserialize)]
struct UpdateConversationRequest {
    title: Option<String>,
    permission_mode: Option<PermissionMode>,
}

/// `PATCH /api/v1/conversations/:id` — update conversation fields.
async fn update_conversation(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<UpdateConversationRequest>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let repo = PgConversationRepo::new(state.db.clone());
    let conversation_id = ConversationId::from_uuid(id);

    // Verify ownership.
    let conversation = repo.get_by_id(conversation_id).await?;
    if conversation.user_id != auth_user.user_id {
        return Err(AppError::NotFound("conversation".into()));
    }

    if let Some(ref title) = body.title {
        repo.update_title(conversation_id, title).await?;
    }
    if let Some(mode) = body.permission_mode {
        repo.update_permission_mode(conversation_id, mode).await?;
    }

    // Re-fetch to return current state.
    let updated = repo.get_by_id(conversation_id).await?;

    Ok(ApiResponse::new(serde_json::json!({
        "id": updated.id.to_string(),
        "title": updated.title,
        "permission_mode": permission_mode_str(updated.permission_mode),
    })))
}

/// `DELETE /api/v1/conversations/:id` — delete a conversation.
async fn delete_conversation(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let repo = PgConversationRepo::new(state.db.clone());
    let conversation_id = ConversationId::from_uuid(id);

    // Verify ownership.
    let conversation = repo.get_by_id(conversation_id).await?;
    if conversation.user_id != auth_user.user_id {
        return Err(AppError::NotFound("conversation".into()));
    }

    repo.delete(conversation_id).await?;

    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}
