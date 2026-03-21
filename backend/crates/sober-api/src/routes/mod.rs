//! Route handlers and router assembly.

pub mod auth;
pub mod collaborators;
pub mod conversations;
pub mod health;
pub mod messages;
pub mod plugins;
pub mod tags;
pub mod users;
pub mod workspaces;
pub mod ws;

use std::sync::Arc;

use axum::Router;
use sober_auth::AuthLayer;
use sober_core::error::AppError;
use sober_core::types::{
    ConversationId, ConversationUser, ConversationUserRepo, CreateMessage, Message, MessageRepo,
    MessageRole, UserId,
};
use sober_db::{PgConversationUserRepo, PgMessageRepo, PgRoleRepo, PgSessionRepo, PgUserRepo};
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

/// Inserts a timeline event message into a conversation.
pub(crate) async fn insert_event_message(
    msg_repo: &PgMessageRepo,
    conversation_id: ConversationId,
    content: &str,
    metadata: serde_json::Value,
) -> Result<Message, AppError> {
    msg_repo
        .create(CreateMessage {
            conversation_id,
            role: MessageRole::Event,
            content: content.to_string(),
            tool_calls: None,
            tool_result: None,
            token_count: None,
            metadata: Some(metadata),
            user_id: None,
        })
        .await
}

use crate::state::AppState;

/// Builds the complete API router with all routes and middleware.
pub fn build_router(state: Arc<AppState>) -> Router {
    let auth_layer = AuthLayer::<PgUserRepo, PgSessionRepo, PgRoleRepo>::new(state.auth.clone());

    let api = Router::new()
        .merge(health::routes())
        .merge(auth::routes())
        .merge(conversations::routes())
        .merge(collaborators::routes())
        .merge(messages::routes())
        .merge(plugins::routes())
        .merge(tags::routes())
        .merge(users::routes())
        .merge(workspaces::routes())
        .merge(ws::routes())
        .layer(auth_layer)
        .with_state(state);

    Router::new().nest("/api/v1", api)
}
