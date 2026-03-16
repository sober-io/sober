//! Conversation CRUD route handlers.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use sober_auth::AuthUser;
use sober_core::PermissionMode;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, ConversationId, ConversationKind, ConversationRepo, ConversationUserRepo,
    ConversationWithDetails, JobRepo, ListConversationsFilter, MessageRepo, TagRepo, WorkspaceId,
};
use sober_db::{PgConversationRepo, PgConversationUserRepo, PgJobRepo, PgMessageRepo, PgTagRepo};

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
        .route("/conversations/{id}/read", post(mark_read))
        .route("/conversations/{id}/messages", delete(clear_messages))
        .route("/conversations/{id}/jobs", get(list_conversation_jobs))
}

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
    let repo = PgConversationRepo::new(state.db.clone());
    let filter = ListConversationsFilter {
        archived: query.archived,
        kind: query.kind,
        tag: query.tag,
        search: query.search,
    };
    let conversations = repo.list_with_details(auth_user.user_id, filter).await?;

    Ok(ApiResponse::new(conversations))
}

/// Request body for `POST /conversations`.
#[derive(Deserialize)]
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
        "kind": conversation.kind,
        "is_archived": conversation.is_archived,
        "permission_mode": conversation.permission_mode.as_str(),
        "created_at": conversation.created_at.to_rfc3339(),
        "updated_at": conversation.updated_at.to_rfc3339(),
    })))
}

/// `GET /api/v1/conversations/:id` — get a conversation with details.
async fn get_conversation(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<ConversationWithDetails>, AppError> {
    let conv_repo = PgConversationRepo::new(state.db.clone());
    let cu_repo = PgConversationUserRepo::new(state.db.clone());

    let conversation_id = ConversationId::from_uuid(id);
    let conversation = conv_repo.get_by_id(conversation_id).await?;

    // Verify ownership.
    if conversation.user_id != auth_user.user_id {
        return Err(AppError::NotFound("conversation".into()));
    }

    // Get the user's unread count.
    let cu = cu_repo.get(conversation_id, auth_user.user_id).await?;

    // Get all users in this conversation.
    let users = cu_repo.list_by_conversation(conversation_id).await?;

    let tag_repo = PgTagRepo::new(state.db.clone());
    let tags = tag_repo.list_by_conversation(conversation_id).await?;

    let details = ConversationWithDetails {
        conversation,
        unread_count: cu.unread_count,
        tags,
        users,
    };

    Ok(ApiResponse::new(details))
}

/// Request body for `PATCH /conversations/:id`.
#[derive(Deserialize)]
struct UpdateConversationRequest {
    title: Option<String>,
    permission_mode: Option<PermissionMode>,
    archived: Option<bool>,
    #[serde(default)]
    workspace_id: Option<Option<String>>,
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
    if let Some(archived) = body.archived {
        repo.update_archived(conversation_id, archived).await?;
    }
    if let Some(ws_id) = body.workspace_id {
        let workspace_id = ws_id
            .map(|s| {
                s.parse::<uuid::Uuid>()
                    .map(WorkspaceId::from_uuid)
                    .map_err(|_| AppError::Validation("invalid workspace_id".into()))
            })
            .transpose()?;
        repo.update_workspace(conversation_id, workspace_id).await?;
    }

    // Re-fetch to return current state.
    let updated = repo.get_by_id(conversation_id).await?;

    Ok(ApiResponse::new(serde_json::json!({
        "id": updated.id.to_string(),
        "title": updated.title,
        "kind": updated.kind,
        "is_archived": updated.is_archived,
        "permission_mode": updated.permission_mode.as_str(),
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

/// `GET /api/v1/conversations/inbox` — get the user's inbox conversation.
async fn get_inbox(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let repo = PgConversationRepo::new(state.db.clone());
    let conv = repo.get_inbox(auth_user.user_id).await?;

    Ok(ApiResponse::new(serde_json::json!({
        "id": conv.id.to_string(),
        "title": conv.title,
        "kind": conv.kind,
        "is_archived": conv.is_archived,
        "permission_mode": conv.permission_mode.as_str(),
        "created_at": conv.created_at.to_rfc3339(),
        "updated_at": conv.updated_at.to_rfc3339(),
    })))
}

/// `POST /api/v1/conversations/:id/read` — mark conversation as read.
async fn mark_read(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let cu_repo = PgConversationUserRepo::new(state.db.clone());
    let conversation_id = ConversationId::from_uuid(id);
    cu_repo
        .mark_read(conversation_id, auth_user.user_id)
        .await?;

    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}

/// `GET /api/v1/conversations/:id/jobs` — list jobs linked to a conversation.
async fn list_conversation_jobs(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<impl axum::response::IntoResponse, AppError> {
    let conv_repo = PgConversationRepo::new(state.db.clone());
    let conversation_id = ConversationId::from_uuid(id);
    let conv = conv_repo.get_by_id(conversation_id).await?;
    if conv.user_id != auth_user.user_id {
        return Err(AppError::NotFound("conversation not found".into()));
    }
    let job_repo = PgJobRepo::new(state.db.clone());
    let jobs = job_repo
        .list_filtered(None, None, &[], None, None, Some(id))
        .await?;
    Ok(ApiResponse::new(jobs))
}

/// `DELETE /api/v1/conversations/:id/messages` — clear all messages in a conversation.
async fn clear_messages(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let conv_repo = PgConversationRepo::new(state.db.clone());
    let conversation_id = ConversationId::from_uuid(id);

    // Verify ownership.
    let conv = conv_repo.get_by_id(conversation_id).await?;
    if conv.user_id != auth_user.user_id {
        return Err(AppError::NotFound("conversation not found".into()));
    }

    let msg_repo = PgMessageRepo::new(state.db.clone());
    let cu_repo = PgConversationUserRepo::new(state.db.clone());
    msg_repo.clear_conversation(conversation_id).await?;
    cu_repo.reset_all_unread(conversation_id).await?;

    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}
