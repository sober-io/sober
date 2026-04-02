//! Tag CRUD and conversation tagging route handlers.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{ApiResponse, ConversationId, Tag, TagId};

use crate::state::AppState;

/// Returns the tag routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tags", get(list_tags))
        .route("/conversations/{id}/tags", post(add_conversation_tag))
        .route(
            "/conversations/{id}/tags/{tag_id}",
            delete(remove_conversation_tag),
        )
}

/// `GET /api/v1/tags` — list all tags for the authenticated user.
async fn list_tags(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
) -> Result<ApiResponse<Vec<Tag>>, AppError> {
    let tags = state.tag.list_by_user(auth_user.user_id).await?;
    Ok(ApiResponse::new(tags))
}

/// Request body for `POST /conversations/:id/tags`.
#[derive(Deserialize)]
struct AddTagRequest {
    name: String,
}

/// `POST /api/v1/conversations/:id/tags` — add a tag to a conversation.
async fn add_conversation_tag(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<AddTagRequest>,
) -> Result<ApiResponse<Tag>, AppError> {
    let tag = state
        .tag
        .add_to_conversation(ConversationId::from_uuid(id), auth_user.user_id, body.name)
        .await?;
    Ok(ApiResponse::new(tag))
}

/// `DELETE /api/v1/conversations/:id/tags/:tag_id` — remove a tag from a conversation.
async fn remove_conversation_tag(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path((id, tag_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state
        .tag
        .remove_from_conversation(
            ConversationId::from_uuid(id),
            auth_user.user_id,
            TagId::from_uuid(tag_id),
        )
        .await?;
    Ok(ApiResponse::new(serde_json::json!({ "removed": true })))
}
