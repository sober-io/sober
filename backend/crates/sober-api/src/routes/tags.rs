//! Tag CRUD and conversation tagging route handlers.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, ConversationId, ConversationRepo, CreateTag, Tag, TagId, TagRepo,
};
use sober_db::{PgConversationRepo, PgTagRepo};

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
    let tag_repo = PgTagRepo::new(state.db.clone());
    let tags = tag_repo.list_by_user(auth_user.user_id).await?;
    Ok(ApiResponse::new(tags))
}

/// Request body for `POST /conversations/:id/tags`.
#[derive(Deserialize)]
struct AddTagRequest {
    name: String,
}

/// `POST /api/v1/conversations/:id/tags` — add a tag to a conversation.
///
/// Creates the tag if it does not already exist (idempotent by name), then
/// attaches it to the conversation. Color is assigned deterministically by
/// the repository based on the tag name.
async fn add_conversation_tag(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<AddTagRequest>,
) -> Result<ApiResponse<Tag>, AppError> {
    let conv_repo = PgConversationRepo::new(state.db.clone());
    let tag_repo = PgTagRepo::new(state.db.clone());

    let conversation_id = ConversationId::from_uuid(id);

    // Verify conversation ownership.
    let conversation = conv_repo.get_by_id(conversation_id).await?;
    if conversation.user_id != auth_user.user_id {
        return Err(AppError::NotFound("conversation".into()));
    }

    let tag = tag_repo
        .create_or_get(CreateTag {
            user_id: auth_user.user_id,
            name: body.name,
        })
        .await?;

    tag_repo.tag_conversation(conversation_id, tag.id).await?;

    Ok(ApiResponse::new(tag))
}

/// `DELETE /api/v1/conversations/:id/tags/:tag_id` — remove a tag from a conversation.
async fn remove_conversation_tag(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path((id, tag_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let conv_repo = PgConversationRepo::new(state.db.clone());
    let tag_repo = PgTagRepo::new(state.db.clone());

    let conversation_id = ConversationId::from_uuid(id);
    let tag_id = TagId::from_uuid(tag_id);

    // Verify conversation ownership.
    let conversation = conv_repo.get_by_id(conversation_id).await?;
    if conversation.user_id != auth_user.user_id {
        return Err(AppError::NotFound("conversation".into()));
    }

    tag_repo.untag_conversation(conversation_id, tag_id).await?;

    Ok(ApiResponse::new(serde_json::json!({ "removed": true })))
}
