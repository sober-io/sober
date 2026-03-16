//! Member management route handlers for group conversations.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, ConversationId, ConversationRepo, ConversationUserRepo, ConversationUserRole,
    ConversationUserWithUsername, CreateMessage, Message, MessageRepo, MessageRole, UserId,
    UserRepo,
};
use sober_db::{PgConversationRepo, PgConversationUserRepo, PgMessageRepo, PgUserRepo};

use crate::routes::ws::{MemberInfo, ServerWsMessage};
use crate::state::AppState;

/// Returns the member management routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/conversations/{id}/members",
            get(list_members).post(add_member),
        )
        .route(
            "/conversations/{id}/members/{user_id}",
            patch(update_role).delete(remove_member),
        )
        .route("/conversations/{id}/leave", post(leave))
}

/// `GET /api/v1/conversations/:id/members` — list members.
async fn list_members(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<Vec<ConversationUserWithUsername>>, AppError> {
    let cu_repo = PgConversationUserRepo::new(state.db.clone());
    let conversation_id = ConversationId::from_uuid(id);

    // Verify the caller is a member.
    cu_repo.get(conversation_id, auth_user.user_id).await?;

    let members = cu_repo.list_members(conversation_id).await?;
    Ok(ApiResponse::new(members))
}

/// Request body for `POST /conversations/:id/members`.
#[derive(Deserialize)]
struct AddMemberRequest {
    username: String,
}

/// `POST /api/v1/conversations/:id/members` — add a member.
async fn add_member(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<AddMemberRequest>,
) -> Result<ApiResponse<ConversationUserWithUsername>, AppError> {
    let cu_repo = PgConversationUserRepo::new(state.db.clone());
    let user_repo = PgUserRepo::new(state.db.clone());
    let msg_repo = PgMessageRepo::new(state.db.clone());
    let conversation_id = ConversationId::from_uuid(id);

    // Auth: caller must be owner or admin.
    let caller_cu = cu_repo.get(conversation_id, auth_user.user_id).await?;
    if caller_cu.role != ConversationUserRole::Owner
        && caller_cu.role != ConversationUserRole::Admin
    {
        return Err(AppError::Forbidden);
    }

    // Look up target user by username.
    let target_user = user_repo.get_by_username(&body.username).await?;

    // Idempotent: if already a member, return existing membership.
    if cu_repo.get(conversation_id, target_user.id).await.is_ok() {
        let members = cu_repo.list_members(conversation_id).await?;
        let existing = members
            .into_iter()
            .find(|m| m.user_id == target_user.id)
            .ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!("member not found after get").into())
            })?;
        return Ok(ApiResponse::new(existing));
    }

    // Add member.
    cu_repo
        .create(
            conversation_id,
            target_user.id,
            ConversationUserRole::Member,
        )
        .await?;

    // Insert event message.
    let actor = user_repo.get_by_id(auth_user.user_id).await?;
    let content = format!("{} added {}", actor.username, target_user.username);
    let metadata = serde_json::json!({
        "type": "member_added",
        "actor_id": auth_user.user_id.to_string(),
        "target_id": target_user.id.to_string(),
        "target_username": target_user.username,
        "role": "member"
    });
    insert_event_message(&msg_repo, conversation_id, &content, metadata).await?;

    // Return the new member with username.
    let members = cu_repo.list_members(conversation_id).await?;
    let new_member = members
        .iter()
        .find(|m| m.user_id == target_user.id)
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("member not found after create").into()))?
        .clone();

    // Broadcast the member_added event to all current members.
    let ws_msg = ServerWsMessage::ChatMemberAdded {
        conversation_id: conversation_id.to_string(),
        user: MemberInfo {
            id: target_user.id.to_string(),
            username: target_user.username.clone(),
        },
        role: "member".to_string(),
    };
    for member in &members {
        state
            .user_connections
            .send(&member.user_id.to_string(), ws_msg.clone())
            .await;
    }

    Ok(ApiResponse::new(new_member))
}

/// Request body for `PATCH /conversations/:id/members/:user_id`.
#[derive(Deserialize)]
struct UpdateRoleRequest {
    role: ConversationUserRole,
}

/// `PATCH /api/v1/conversations/:id/members/:user_id` — change a member's role.
async fn update_role(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path((id, target_user_id)): Path<(uuid::Uuid, uuid::Uuid)>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let cu_repo = PgConversationUserRepo::new(state.db.clone());
    let user_repo = PgUserRepo::new(state.db.clone());
    let msg_repo = PgMessageRepo::new(state.db.clone());
    let conversation_id = ConversationId::from_uuid(id);
    let target_id = UserId::from_uuid(target_user_id);

    // Cannot set role to owner.
    if body.role == ConversationUserRole::Owner {
        return Err(AppError::Validation("cannot set role to owner".into()));
    }

    // Auth: only owner can change roles.
    let caller_cu = cu_repo.get(conversation_id, auth_user.user_id).await?;
    if caller_cu.role != ConversationUserRole::Owner {
        return Err(AppError::Forbidden);
    }

    // Verify target is a member.
    let target_cu = cu_repo.get(conversation_id, target_id).await?;

    // Only owner->admin and admin->member transitions allowed (plus member->admin).
    // Cannot change owner's role.
    if target_cu.role == ConversationUserRole::Owner {
        return Err(AppError::Validation("cannot change owner's role".into()));
    }

    cu_repo
        .update_role(conversation_id, target_id, body.role)
        .await?;

    // Insert event message.
    let actor = user_repo.get_by_id(auth_user.user_id).await?;
    let target_user = user_repo.get_by_id(target_id).await?;
    let role_str = match body.role {
        ConversationUserRole::Admin => "admin",
        ConversationUserRole::Member => "member",
        ConversationUserRole::Owner => unreachable!(),
    };
    let content = format!(
        "{} changed {}'s role to {}",
        actor.username, target_user.username, role_str
    );
    let metadata = serde_json::json!({
        "type": "role_changed",
        "actor_id": auth_user.user_id.to_string(),
        "target_id": target_id.to_string(),
        "target_username": target_user.username,
        "role": role_str
    });
    insert_event_message(&msg_repo, conversation_id, &content, metadata).await?;

    // Broadcast the role_changed event to all members.
    let members = cu_repo.list_by_conversation(conversation_id).await?;
    let ws_msg = ServerWsMessage::ChatRoleChanged {
        conversation_id: conversation_id.to_string(),
        user_id: target_id.to_string(),
        role: role_str.to_string(),
    };
    for member in &members {
        state
            .user_connections
            .send(&member.user_id.to_string(), ws_msg.clone())
            .await;
    }

    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}

/// `DELETE /api/v1/conversations/:id/members/:user_id` — kick a member.
async fn remove_member(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path((id, target_user_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let cu_repo = PgConversationUserRepo::new(state.db.clone());
    let user_repo = PgUserRepo::new(state.db.clone());
    let msg_repo = PgMessageRepo::new(state.db.clone());
    let conversation_id = ConversationId::from_uuid(id);
    let target_id = UserId::from_uuid(target_user_id);

    // Auth check.
    let caller_cu = cu_repo.get(conversation_id, auth_user.user_id).await?;
    let target_cu = cu_repo.get(conversation_id, target_id).await?;

    // Cannot kick the owner.
    if target_cu.role == ConversationUserRole::Owner {
        return Err(AppError::Forbidden);
    }

    match caller_cu.role {
        ConversationUserRole::Owner => {
            // Owner can kick anyone (except owner, handled above).
        }
        ConversationUserRole::Admin => {
            // Admin can only kick members, not other admins.
            if target_cu.role != ConversationUserRole::Member {
                return Err(AppError::Forbidden);
            }
        }
        ConversationUserRole::Member => {
            return Err(AppError::Forbidden);
        }
    }

    // Collect remaining members before removal for broadcasting.
    let remaining_members = cu_repo.list_by_conversation(conversation_id).await?;

    cu_repo.remove_member(conversation_id, target_id).await?;

    // Insert event message.
    let actor = user_repo.get_by_id(auth_user.user_id).await?;
    let target_user = user_repo.get_by_id(target_id).await?;
    let content = format!("{} removed {}", actor.username, target_user.username);
    let metadata = serde_json::json!({
        "type": "member_removed",
        "actor_id": auth_user.user_id.to_string(),
        "target_id": target_id.to_string(),
        "target_username": target_user.username
    });
    insert_event_message(&msg_repo, conversation_id, &content, metadata).await?;

    // Broadcast the member_removed event to all remaining members.
    let ws_msg = ServerWsMessage::ChatMemberRemoved {
        conversation_id: conversation_id.to_string(),
        user_id: target_id.to_string(),
    };
    for member in &remaining_members {
        state
            .user_connections
            .send(&member.user_id.to_string(), ws_msg.clone())
            .await;
    }
    // Also notify the kicked user (they are no longer in remaining_members).
    state
        .user_connections
        .send(&target_id.to_string(), ws_msg)
        .await;

    // Auto-convert back to direct if only the owner remains.
    let current_members = cu_repo.list_by_conversation(conversation_id).await?;
    if current_members.len() == 1 {
        let conv_repo = PgConversationRepo::new(state.db.clone());
        conv_repo.convert_to_direct(conversation_id).await.ok();
    }

    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}

/// `POST /api/v1/conversations/:id/leave` — leave a conversation.
async fn leave(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let cu_repo = PgConversationUserRepo::new(state.db.clone());
    let user_repo = PgUserRepo::new(state.db.clone());
    let msg_repo = PgMessageRepo::new(state.db.clone());
    let conversation_id = ConversationId::from_uuid(id);

    let caller_cu = cu_repo.get(conversation_id, auth_user.user_id).await?;

    // Owner cannot leave.
    if caller_cu.role == ConversationUserRole::Owner {
        return Err(AppError::Forbidden);
    }

    // Collect remaining members before removal for broadcasting.
    let remaining_members = cu_repo.list_by_conversation(conversation_id).await?;

    cu_repo
        .remove_member(conversation_id, auth_user.user_id)
        .await?;

    // Insert event message.
    let user = user_repo.get_by_id(auth_user.user_id).await?;
    let content = format!("{} left", user.username);
    let metadata = serde_json::json!({
        "type": "member_left",
        "actor_id": auth_user.user_id.to_string()
    });
    insert_event_message(&msg_repo, conversation_id, &content, metadata).await?;

    // Broadcast the member_removed event to all remaining members.
    let ws_msg = ServerWsMessage::ChatMemberRemoved {
        conversation_id: conversation_id.to_string(),
        user_id: auth_user.user_id.to_string(),
    };
    for member in &remaining_members {
        state
            .user_connections
            .send(&member.user_id.to_string(), ws_msg.clone())
            .await;
    }
    // Also notify the leaving user themselves.
    state
        .user_connections
        .send(&auth_user.user_id.to_string(), ws_msg)
        .await;

    // Auto-convert back to direct if only the owner remains.
    let current_members = cu_repo.list_by_conversation(conversation_id).await?;
    if current_members.len() == 1 {
        let conv_repo = PgConversationRepo::new(state.db.clone());
        conv_repo.convert_to_direct(conversation_id).await.ok();
    }

    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}

/// Inserts a timeline event message into a conversation.
async fn insert_event_message(
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
        })
        .await
}
