//! Message route handlers: pagination, deletion, and tagging.

use std::sync::Arc;

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, ConversationId, CreateTag, Message, MessageId, MessageRepo, Tag, TagId, TagRepo,
};
use sober_db::{PgMessageRepo, PgTagRepo};

use crate::state::AppState;

/// Returns the message routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/conversations/{id}/messages", get(list_messages))
        .route("/conversations/{id}/message-tags", get(list_message_tags))
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
    let msg_repo = PgMessageRepo::new(state.db.clone());

    // Verify membership.
    let _membership = super::verify_membership(&state.db, id, auth_user.user_id).await?;

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

    let msg = msg_repo.get_by_id(id).await?;

    // Verify membership.
    let membership =
        super::verify_membership(&state.db, msg.conversation_id, auth_user.user_id).await?;

    let is_owner = membership.role == sober_core::types::ConversationUserRole::Owner;
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
    let tag_repo = PgTagRepo::new(state.db.clone());

    // Verify membership.
    let msg = msg_repo.get_by_id(id).await?;
    let _membership =
        super::verify_membership(&state.db, msg.conversation_id, auth_user.user_id).await?;

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
    let tag_repo = PgTagRepo::new(state.db.clone());

    // Verify membership.
    let msg = msg_repo.get_by_id(id).await?;
    let _membership =
        super::verify_membership(&state.db, msg.conversation_id, auth_user.user_id).await?;

    tag_repo.untag_message(id, tag_id).await?;
    Ok(ApiResponse::new(serde_json::json!({ "removed": true })))
}

/// `GET /api/v1/conversations/:id/message-tags` — get all message tags for a conversation.
///
/// Returns a map of `message_id → [Tag]` for every tagged message in the conversation.
async fn list_message_tags(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<ConversationId>,
) -> Result<ApiResponse<HashMap<String, Vec<Tag>>>, AppError> {
    let tag_repo = PgTagRepo::new(state.db.clone());

    // Verify membership.
    let _membership = super::verify_membership(&state.db, id, auth_user.user_id).await?;

    let pairs = tag_repo.list_by_conversation_messages(id).await?;

    let mut map: HashMap<String, Vec<Tag>> = HashMap::new();
    for (message_id, tag) in pairs {
        map.entry(message_id.to_string()).or_default().push(tag);
    }

    Ok(ApiResponse::new(map))
}
