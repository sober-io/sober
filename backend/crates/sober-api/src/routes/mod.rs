//! Route handlers and router assembly.

pub mod auth;
pub mod conversations;
pub mod health;
pub mod mcp;
pub mod members;
pub mod messages;
pub mod tags;
pub mod users;
pub mod workspaces;
pub mod ws;

use std::sync::Arc;

use axum::Router;
use sober_auth::AuthLayer;
use sober_core::error::AppError;
use sober_core::types::{ConversationId, ConversationUser, ConversationUserRepo, UserId};
use sober_db::{PgConversationUserRepo, PgRoleRepo, PgSessionRepo, PgUserRepo};
use sqlx::PgPool;

/// Verify the authenticated user is a member of the conversation.
/// Returns the membership info, or `NotFound` if not a member.
pub async fn verify_membership(
    db: &PgPool,
    conversation_id: ConversationId,
    user_id: UserId,
) -> Result<ConversationUser, AppError> {
    let cu_repo = PgConversationUserRepo::new(db.clone());
    cu_repo.get(conversation_id, user_id).await
}

use crate::state::AppState;

/// Builds the complete API router with all routes and middleware.
pub fn build_router(state: Arc<AppState>) -> Router {
    let auth_layer = AuthLayer::<PgUserRepo, PgSessionRepo, PgRoleRepo>::new(state.auth.clone());

    let api = Router::new()
        .merge(health::routes())
        .merge(auth::routes())
        .merge(conversations::routes())
        .merge(members::routes())
        .merge(messages::routes())
        .merge(mcp::routes())
        .merge(tags::routes())
        .merge(users::routes())
        .merge(workspaces::routes())
        .merge(ws::routes())
        .layer(auth_layer)
        .with_state(state);

    Router::new().nest("/api/v1", api)
}
