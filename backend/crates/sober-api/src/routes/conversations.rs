//! Conversation CRUD route handlers.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{
    AgentMode, ApiResponse, ConversationId, ConversationKind, ConversationWithDetails, MessageId,
    PermissionMode, SandboxNetMode,
};

use crate::services::conversation::{
    ConvertToGroupResponse, CreateConversationResponse, InboxResponse, SettingsResponse,
    UpdateConversationResponse, UpdateSettingsInput,
};
use crate::state::AppState;

/// Returns the conversation routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/conversations/inbox", get(get_inbox))
        .route(
            "/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route(
            "/conversations/{id}",
            get(get_conversation)
                .patch(update_conversation)
                .delete(delete_conversation),
        )
        .route(
            "/conversations/{id}/settings",
            get(get_settings).patch(update_settings),
        )
        .route("/conversations/{id}/read", post(mark_read))
        .route("/conversations/{id}/messages", delete(clear_messages))
        .route(
            "/conversations/{id}/convert-to-group",
            post(convert_to_group),
        )
        .route("/conversations/{id}/jobs", get(list_conversation_jobs))
}

// ---------------------------------------------------------------------------
// List / Create / Get / Update / Delete conversations
// ---------------------------------------------------------------------------

/// Query parameters for `GET /conversations`.
#[derive(Deserialize)]
struct ListConversationsQuery {
    archived: Option<bool>,
    kind: Option<ConversationKind>,
    tag: Option<String>,
    search: Option<String>,
}

/// `GET /api/v1/conversations` — list conversations for the authenticated user.
async fn list_conversations(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Query(query): Query<ListConversationsQuery>,
) -> Result<ApiResponse<Vec<ConversationWithDetails>>, AppError> {
    let filter = sober_core::types::ListConversationsFilter {
        archived: query.archived,
        kind: query.kind,
        tag: query.tag,
        search: query.search,
    };
    let conversations = state.conversation.list(auth_user.user_id, filter).await?;
    Ok(ApiResponse::new(conversations))
}

/// Request body for `POST /conversations`.
#[derive(Deserialize)]
struct CreateConversationRequest {
    title: Option<String>,
}

/// `POST /api/v1/conversations` — create a new direct conversation.
async fn create_conversation(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Json(body): Json<CreateConversationRequest>,
) -> Result<ApiResponse<CreateConversationResponse>, AppError> {
    let response = state
        .conversation
        .create(auth_user.user_id, body.title.as_deref())
        .await?;
    Ok(ApiResponse::new(response))
}

/// `GET /api/v1/conversations/:id` — get a conversation with details.
async fn get_conversation(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<ConversationWithDetails>, AppError> {
    let details = state
        .conversation
        .get(ConversationId::from_uuid(id), auth_user.user_id)
        .await?;
    Ok(ApiResponse::new(details))
}

/// Request body for `PATCH /conversations/:id`.
#[derive(Deserialize)]
struct UpdateConversationRequest {
    title: Option<String>,
    archived: Option<bool>,
}

/// `PATCH /api/v1/conversations/:id` — update conversation title/archived.
async fn update_conversation(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<UpdateConversationRequest>,
) -> Result<ApiResponse<UpdateConversationResponse>, AppError> {
    let response = state
        .conversation
        .update(
            ConversationId::from_uuid(id),
            auth_user.user_id,
            body.title.as_deref(),
            body.archived,
        )
        .await?;
    Ok(ApiResponse::new(response))
}

/// `DELETE /api/v1/conversations/:id` — delete a conversation.
async fn delete_conversation(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state
        .conversation
        .delete(ConversationId::from_uuid(id), auth_user.user_id)
        .await?;
    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}

// ---------------------------------------------------------------------------
// Settings (GET + PATCH)
// ---------------------------------------------------------------------------

/// Request body for `PATCH /conversations/:id/settings`.
#[derive(Deserialize)]
struct UpdateSettingsRequest {
    permission_mode: Option<PermissionMode>,
    agent_mode: Option<AgentMode>,
    sandbox_profile: Option<String>,
    sandbox_net_mode: Option<SandboxNetMode>,
    sandbox_allowed_domains: Option<Vec<String>>,
    sandbox_max_execution_seconds: Option<i32>,
    sandbox_allow_spawn: Option<bool>,
    auto_snapshot: Option<bool>,
    max_snapshots: Option<i32>,
    disabled_tools: Option<Vec<String>>,
    disabled_plugins: Option<Vec<String>>,
}

/// `GET /api/v1/conversations/:id/settings` — read combined settings.
async fn get_settings(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<SettingsResponse>, AppError> {
    let response = state
        .conversation
        .get_settings(ConversationId::from_uuid(id), auth_user.user_id)
        .await?;
    Ok(ApiResponse::new(response))
}

/// `PATCH /api/v1/conversations/:id/settings` — partial update.
async fn update_settings(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<UpdateSettingsRequest>,
) -> Result<ApiResponse<SettingsResponse>, AppError> {
    let input = UpdateSettingsInput {
        permission_mode: body.permission_mode,
        agent_mode: body.agent_mode,
        sandbox_profile: body.sandbox_profile,
        sandbox_net_mode: body.sandbox_net_mode,
        sandbox_allowed_domains: body.sandbox_allowed_domains,
        sandbox_max_execution_seconds: body.sandbox_max_execution_seconds,
        sandbox_allow_spawn: body.sandbox_allow_spawn,
        auto_snapshot: body.auto_snapshot,
        max_snapshots: body.max_snapshots,
        disabled_tools: body.disabled_tools,
        disabled_plugins: body.disabled_plugins,
    };
    let response = state
        .conversation
        .update_settings(ConversationId::from_uuid(id), auth_user.user_id, input)
        .await?;
    Ok(ApiResponse::new(response))
}

// ---------------------------------------------------------------------------
// Inbox / Read / Clear / Convert / Jobs
// ---------------------------------------------------------------------------

/// `GET /api/v1/conversations/inbox` — get the user's inbox conversation.
async fn get_inbox(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
) -> Result<ApiResponse<InboxResponse>, AppError> {
    let response = state.conversation.get_inbox(auth_user.user_id).await?;
    Ok(ApiResponse::new(response))
}

/// Request body for `mark_read`.
#[derive(Deserialize)]
struct MarkReadRequest {
    message_id: Option<uuid::Uuid>,
}

/// `POST /api/v1/conversations/:id/read` — mark conversation as read.
async fn mark_read(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    body: Option<axum::Json<MarkReadRequest>>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let message_id = body
        .and_then(|axum::Json(req)| req.message_id)
        .map(MessageId::from_uuid);

    state
        .conversation
        .mark_read(ConversationId::from_uuid(id), auth_user.user_id, message_id)
        .await?;

    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}

/// `GET /api/v1/conversations/:id/jobs` — list jobs linked to a conversation.
async fn list_conversation_jobs(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<impl axum::response::IntoResponse, AppError> {
    let jobs = state
        .conversation
        .list_jobs(ConversationId::from_uuid(id), auth_user.user_id)
        .await?;
    Ok(ApiResponse::new(jobs))
}

/// Request body for `POST /conversations/:id/convert-to-group`.
#[derive(Deserialize)]
struct ConvertToGroupRequest {
    title: String,
}

/// `POST /api/v1/conversations/:id/convert-to-group` — convert a direct conversation to a group.
async fn convert_to_group(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<ConvertToGroupRequest>,
) -> Result<ApiResponse<ConvertToGroupResponse>, AppError> {
    let response = state
        .conversation
        .convert_to_group(
            ConversationId::from_uuid(id),
            auth_user.user_id,
            &body.title,
        )
        .await?;
    Ok(ApiResponse::new(response))
}

/// `DELETE /api/v1/conversations/:id/messages` — clear all messages in a conversation.
async fn clear_messages(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state
        .conversation
        .clear_messages(ConversationId::from_uuid(id), auth_user.user_id)
        .await?;
    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}
