//! Collaborator management route handlers for group conversations.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, ConversationId, ConversationUserRole, ConversationUserWithUsername, UserId,
};

use crate::state::AppState;

/// Returns the collaborator management routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/conversations/{id}/collaborators",
            get(list_collaborators).post(add_collaborator),
        )
        .route(
            "/conversations/{id}/collaborators/{user_id}",
            patch(update_collaborator_role).delete(remove_collaborator),
        )
        .route("/conversations/{id}/leave", post(leave))
}

/// `GET /api/v1/conversations/:id/collaborators` — list collaborators.
async fn list_collaborators(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<Vec<ConversationUserWithUsername>>, AppError> {
    let collaborators = state
        .collaborator
        .list(ConversationId::from_uuid(id), auth_user.user_id)
        .await?;
    Ok(ApiResponse::new(collaborators))
}

/// Request body for `POST /conversations/:id/collaborators`.
#[derive(Deserialize)]
struct AddCollaboratorRequest {
    username: String,
}

/// `POST /api/v1/conversations/:id/collaborators` — add a collaborator.
async fn add_collaborator(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<AddCollaboratorRequest>,
) -> Result<ApiResponse<ConversationUserWithUsername>, AppError> {
    let collaborator = state
        .collaborator
        .add(
            ConversationId::from_uuid(id),
            auth_user.user_id,
            &body.username,
        )
        .await?;
    Ok(ApiResponse::new(collaborator))
}

/// Request body for `PATCH /conversations/:id/collaborators/:user_id`.
#[derive(Deserialize)]
struct UpdateRoleRequest {
    role: ConversationUserRole,
}

/// `PATCH /api/v1/conversations/:id/collaborators/:user_id` — change a collaborator's role.
async fn update_collaborator_role(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path((id, target_user_id)): Path<(uuid::Uuid, uuid::Uuid)>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state
        .collaborator
        .update_role(
            ConversationId::from_uuid(id),
            auth_user.user_id,
            UserId::from_uuid(target_user_id),
            body.role,
        )
        .await?;
    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}

/// `DELETE /api/v1/conversations/:id/collaborators/:user_id` — remove a collaborator.
async fn remove_collaborator(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path((id, target_user_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state
        .collaborator
        .remove(
            ConversationId::from_uuid(id),
            auth_user.user_id,
            UserId::from_uuid(target_user_id),
        )
        .await?;
    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}

/// `POST /api/v1/conversations/:id/leave` — leave a conversation.
async fn leave(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state
        .collaborator
        .leave(ConversationId::from_uuid(id), auth_user.user_id)
        .await?;
    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}
