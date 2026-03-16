//! Message route handlers: pagination, deletion, and tagging.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, ConversationId, ConversationRepo, CreateTag, Message, MessageId, MessageRepo, Tag,
    TagId, TagRepo,
};
use sober_db::{PgConversationRepo, PgMessageRepo, PgTagRepo};

use crate::state::AppState;

/// Returns the message routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/conversations/{id}/messages", get(list_messages))
        .route("/messages/{id}", delete(delete_message))
        .route("/messages/{id}/tags", post(add_message_tag))
        .route("/messages/{id}/tags/{tag_id}", delete(remove_message_tag))
}

/// Query parameters for `GET /conversations/:id/messages`.
#[derive(Deserialize)]
struct PaginationParams {
    before: Option<MessageId>,
    limit: Option<i64>,
}

/// `GET /api/v1/conversations/:id/messages` — list messages with cursor pagination.
async fn list_messages(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<ConversationId>,
    Query(params): Query<PaginationParams>,
) -> Result<ApiResponse<Vec<Message>>, AppError> {
    let conv_repo = PgConversationRepo::new(state.db.clone());
    let msg_repo = PgMessageRepo::new(state.db.clone());

    // Verify conversation ownership.
    let conv = conv_repo.get_by_id(id).await?;
    if conv.user_id != auth_user.user_id {
        return Err(AppError::NotFound("conversation not found".into()));
    }

    let limit = params.limit.unwrap_or(50).min(100);
    let messages = msg_repo.list_paginated(id, params.before, limit).await?;
    Ok(ApiResponse::new(messages))
}

/// `DELETE /api/v1/messages/:id` — delete a single message.
///
/// Authorized if the caller owns the conversation OR sent the message.
async fn delete_message(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<MessageId>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let msg_repo = PgMessageRepo::new(state.db.clone());
    let conv_repo = PgConversationRepo::new(state.db.clone());

    let msg = msg_repo.get_by_id(id).await?;
    let conv = conv_repo.get_by_id(msg.conversation_id).await?;

    let is_owner = conv.user_id == auth_user.user_id;
    let is_sender = msg.user_id == Some(auth_user.user_id);
    if !is_owner && !is_sender {
        return Err(AppError::NotFound("message not found".into()));
    }

    msg_repo.delete(id).await?;
    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}

/// Request body for `POST /messages/:id/tags`.
#[derive(Deserialize)]
struct AddTagRequest {
    name: String,
}

/// `POST /api/v1/messages/:id/tags` — add a tag to a message.
async fn add_message_tag(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<MessageId>,
    Json(body): Json<AddTagRequest>,
) -> Result<ApiResponse<Tag>, AppError> {
    let msg_repo = PgMessageRepo::new(state.db.clone());
    let conv_repo = PgConversationRepo::new(state.db.clone());
    let tag_repo = PgTagRepo::new(state.db.clone());

    // Verify conversation ownership.
    let msg = msg_repo.get_by_id(id).await?;
    let conv = conv_repo.get_by_id(msg.conversation_id).await?;
    if conv.user_id != auth_user.user_id {
        return Err(AppError::NotFound("message not found".into()));
    }

    let tag = tag_repo
        .create_or_get(CreateTag {
            user_id: auth_user.user_id,
            name: body.name,
        })
        .await?;

    tag_repo.tag_message(id, tag.id).await?;
    Ok(ApiResponse::new(tag))
}

/// `DELETE /api/v1/messages/:id/tags/:tag_id` — remove a tag from a message.
async fn remove_message_tag(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path((id, tag_id)): Path<(MessageId, TagId)>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let msg_repo = PgMessageRepo::new(state.db.clone());
    let conv_repo = PgConversationRepo::new(state.db.clone());
    let tag_repo = PgTagRepo::new(state.db.clone());

    // Verify conversation ownership.
    let msg = msg_repo.get_by_id(id).await?;
    let conv = conv_repo.get_by_id(msg.conversation_id).await?;
    if conv.user_id != auth_user.user_id {
        return Err(AppError::NotFound("message not found".into()));
    }

    tag_repo.untag_message(id, tag_id).await?;
    Ok(ApiResponse::new(serde_json::json!({ "removed": true })))
}
