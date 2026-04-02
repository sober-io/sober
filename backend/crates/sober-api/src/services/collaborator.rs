use sober_core::error::AppError;
use sober_core::types::{
    ContentBlock, ConversationId, ConversationRepo, ConversationUserRepo, ConversationUserRole,
    ConversationUserWithUsername, CreateMessage, MessageRole, UserId, UserRepo,
};
use sober_db::{PgConversationRepo, PgConversationUserRepo, PgMessageRepo, PgUserRepo};
use sqlx::PgPool;
use sqlx::postgres::PgConnection;

use crate::connections::UserConnectionRegistry;
use crate::ws_types::{CollaboratorInfo, ServerWsMessage};

pub struct CollaboratorService {
    db: PgPool,
    user_connections: UserConnectionRegistry,
}

impl CollaboratorService {
    pub fn new(db: PgPool, user_connections: UserConnectionRegistry) -> Self {
        Self {
            db,
            user_connections,
        }
    }

    /// List all collaborators in a conversation.
    pub async fn list(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> Result<Vec<ConversationUserWithUsername>, AppError> {
        let cu_repo = PgConversationUserRepo::new(self.db.clone());
        cu_repo.get(conversation_id, user_id).await?;
        cu_repo.list_collaborators(conversation_id).await
    }

    /// Add a collaborator to a conversation.
    pub async fn add(
        &self,
        conversation_id: ConversationId,
        caller_user_id: UserId,
        target_username: &str,
    ) -> Result<ConversationUserWithUsername, AppError> {
        let cu_repo = PgConversationUserRepo::new(self.db.clone());
        let user_repo = PgUserRepo::new(self.db.clone());

        // Auth: caller must be owner or admin.
        let caller_cu = cu_repo.get(conversation_id, caller_user_id).await?;
        if caller_cu.role != ConversationUserRole::Owner
            && caller_cu.role != ConversationUserRole::Admin
        {
            return Err(AppError::Forbidden);
        }

        let target_user = user_repo.get_by_username(target_username).await?;

        // Idempotent: if already a collaborator, return existing membership.
        if cu_repo.get(conversation_id, target_user.id).await.is_ok() {
            let collaborators = cu_repo.list_collaborators(conversation_id).await?;
            let existing = collaborators
                .into_iter()
                .find(|m| m.user_id == target_user.id)
                .ok_or_else(|| {
                    AppError::Internal(anyhow::anyhow!("collaborator not found after get").into())
                })?;
            return Ok(existing);
        }

        // Add collaborator + event message atomically.
        let actor = user_repo.get_by_id(caller_user_id).await?;
        let content = format!("{} added {}", actor.username, target_user.username);
        let metadata = serde_json::json!({
            "type": "collaborator_added",
            "actor_id": caller_user_id.to_string(),
            "target_id": target_user.id.to_string(),
            "target_username": target_user.username,
            "role": "member"
        });

        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        PgConversationUserRepo::create_tx(
            &mut tx,
            conversation_id,
            target_user.id,
            ConversationUserRole::Member,
        )
        .await?;

        Self::insert_event_message_tx(&mut tx, conversation_id, &content, metadata).await?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        // Return the new collaborator with username.
        let collaborators = cu_repo.list_collaborators(conversation_id).await?;
        let new_collaborator = collaborators
            .iter()
            .find(|m| m.user_id == target_user.id)
            .ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!("collaborator not found after create").into())
            })?
            .clone();

        // Broadcast outside tx.
        let ws_msg = ServerWsMessage::ChatCollaboratorAdded {
            conversation_id: conversation_id.to_string(),
            user: CollaboratorInfo {
                id: target_user.id.to_string(),
                username: target_user.username.clone(),
            },
            role: "member".to_string(),
        };
        for collaborator in &collaborators {
            self.user_connections
                .send(&collaborator.user_id.to_string(), ws_msg.clone())
                .await;
        }

        Ok(new_collaborator)
    }

    /// Change a collaborator's role.
    pub async fn update_role(
        &self,
        conversation_id: ConversationId,
        caller_user_id: UserId,
        target_user_id: UserId,
        role: ConversationUserRole,
    ) -> Result<(), AppError> {
        let cu_repo = PgConversationUserRepo::new(self.db.clone());
        let user_repo = PgUserRepo::new(self.db.clone());

        if role == ConversationUserRole::Owner {
            return Err(AppError::Validation("cannot set role to owner".into()));
        }

        let caller_cu = cu_repo.get(conversation_id, caller_user_id).await?;
        if caller_cu.role != ConversationUserRole::Owner {
            return Err(AppError::Forbidden);
        }

        let target_cu = cu_repo.get(conversation_id, target_user_id).await?;
        if target_cu.role == ConversationUserRole::Owner {
            return Err(AppError::Validation("cannot change owner's role".into()));
        }

        let actor = user_repo.get_by_id(caller_user_id).await?;
        let target_user = user_repo.get_by_id(target_user_id).await?;
        let role_str = match role {
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
            "actor_id": caller_user_id.to_string(),
            "target_id": target_user_id.to_string(),
            "target_username": target_user.username,
            "role": role_str
        });

        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        PgConversationUserRepo::update_role_tx(&mut tx, conversation_id, target_user_id, role)
            .await?;
        Self::insert_event_message_tx(&mut tx, conversation_id, &content, metadata).await?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        // Broadcast outside tx.
        let collaborators = cu_repo.list_by_conversation(conversation_id).await?;
        let ws_msg = ServerWsMessage::ChatRoleChanged {
            conversation_id: conversation_id.to_string(),
            user_id: target_user_id.to_string(),
            role: role_str.to_string(),
        };
        for collaborator in &collaborators {
            self.user_connections
                .send(&collaborator.user_id.to_string(), ws_msg.clone())
                .await;
        }

        Ok(())
    }

    /// Remove a collaborator from a conversation.
    pub async fn remove(
        &self,
        conversation_id: ConversationId,
        caller_user_id: UserId,
        target_user_id: UserId,
    ) -> Result<(), AppError> {
        let cu_repo = PgConversationUserRepo::new(self.db.clone());
        let user_repo = PgUserRepo::new(self.db.clone());

        let caller_cu = cu_repo.get(conversation_id, caller_user_id).await?;
        let target_cu = cu_repo.get(conversation_id, target_user_id).await?;

        check_can_remove(caller_cu.role, target_cu.role)?;

        let remaining = cu_repo.list_by_conversation(conversation_id).await?;

        let actor = user_repo.get_by_id(caller_user_id).await?;
        let target_user = user_repo.get_by_id(target_user_id).await?;
        let content = format!("{} removed {}", actor.username, target_user.username);
        let metadata = serde_json::json!({
            "type": "collaborator_removed",
            "actor_id": caller_user_id.to_string(),
            "target_id": target_user_id.to_string(),
            "target_username": target_user.username
        });

        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        PgConversationUserRepo::remove_collaborator_tx(&mut tx, conversation_id, target_user_id)
            .await?;
        Self::insert_event_message_tx(&mut tx, conversation_id, &content, metadata).await?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        // Broadcast outside tx.
        let ws_msg = ServerWsMessage::ChatCollaboratorRemoved {
            conversation_id: conversation_id.to_string(),
            user_id: target_user_id.to_string(),
        };
        for collaborator in &remaining {
            self.user_connections
                .send(&collaborator.user_id.to_string(), ws_msg.clone())
                .await;
        }
        // Also notify the kicked user.
        self.user_connections
            .send(&target_user_id.to_string(), ws_msg)
            .await;

        // Auto-convert back to direct if only the owner remains.
        let current = cu_repo.list_by_conversation(conversation_id).await?;
        if current.len() == 1 {
            PgConversationRepo::new(self.db.clone())
                .convert_to_direct(conversation_id)
                .await
                .ok();
        }

        Ok(())
    }

    /// Leave a conversation (non-owner only).
    pub async fn leave(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> Result<(), AppError> {
        let cu_repo = PgConversationUserRepo::new(self.db.clone());
        let user_repo = PgUserRepo::new(self.db.clone());

        let caller_cu = cu_repo.get(conversation_id, user_id).await?;
        if caller_cu.role == ConversationUserRole::Owner {
            return Err(AppError::Forbidden);
        }

        let remaining = cu_repo.list_by_conversation(conversation_id).await?;

        let user = user_repo.get_by_id(user_id).await?;
        let content = format!("{} left", user.username);
        let metadata = serde_json::json!({
            "type": "collaborator_left",
            "actor_id": user_id.to_string()
        });

        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        PgConversationUserRepo::remove_collaborator_tx(&mut tx, conversation_id, user_id).await?;
        Self::insert_event_message_tx(&mut tx, conversation_id, &content, metadata).await?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        // Broadcast outside tx.
        let ws_msg = ServerWsMessage::ChatCollaboratorRemoved {
            conversation_id: conversation_id.to_string(),
            user_id: user_id.to_string(),
        };
        for collaborator in &remaining {
            self.user_connections
                .send(&collaborator.user_id.to_string(), ws_msg.clone())
                .await;
        }
        self.user_connections
            .send(&user_id.to_string(), ws_msg)
            .await;

        // Auto-convert back to direct if only the owner remains.
        let current = cu_repo.list_by_conversation(conversation_id).await?;
        if current.len() == 1 {
            PgConversationRepo::new(self.db.clone())
                .convert_to_direct(conversation_id)
                .await
                .ok();
        }

        Ok(())
    }

    /// Insert a timeline event message within a transaction.
    async fn insert_event_message_tx(
        conn: &mut PgConnection,
        conversation_id: ConversationId,
        content: &str,
        metadata: serde_json::Value,
    ) -> Result<sober_core::types::Message, AppError> {
        PgMessageRepo::create_tx(
            conn,
            CreateMessage {
                conversation_id,
                role: MessageRole::Event,
                content: vec![ContentBlock::text(content)],
                reasoning: None,
                token_count: None,
                metadata: Some(metadata),
                user_id: None,
            },
        )
        .await
    }
}

/// Check whether `caller_role` is allowed to remove a user with `target_role`.
fn check_can_remove(
    caller_role: ConversationUserRole,
    target_role: ConversationUserRole,
) -> Result<(), AppError> {
    // Cannot remove the owner.
    if target_role == ConversationUserRole::Owner {
        return Err(AppError::Forbidden);
    }
    match caller_role {
        ConversationUserRole::Owner => Ok(()),
        ConversationUserRole::Admin => {
            if target_role != ConversationUserRole::Member {
                return Err(AppError::Forbidden);
            }
            Ok(())
        }
        ConversationUserRole::Member => Err(AppError::Forbidden),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_can_remove_admin() {
        assert!(check_can_remove(ConversationUserRole::Owner, ConversationUserRole::Admin).is_ok());
    }

    #[test]
    fn owner_can_remove_member() {
        assert!(
            check_can_remove(ConversationUserRole::Owner, ConversationUserRole::Member).is_ok()
        );
    }

    #[test]
    fn nobody_can_remove_owner() {
        assert!(
            check_can_remove(ConversationUserRole::Owner, ConversationUserRole::Owner).is_err()
        );
        assert!(
            check_can_remove(ConversationUserRole::Admin, ConversationUserRole::Owner).is_err()
        );
        assert!(
            check_can_remove(ConversationUserRole::Member, ConversationUserRole::Owner).is_err()
        );
    }

    #[test]
    fn admin_can_remove_member() {
        assert!(
            check_can_remove(ConversationUserRole::Admin, ConversationUserRole::Member).is_ok()
        );
    }

    #[test]
    fn admin_cannot_remove_admin() {
        assert!(
            check_can_remove(ConversationUserRole::Admin, ConversationUserRole::Admin).is_err()
        );
    }

    #[test]
    fn member_cannot_remove_anyone() {
        assert!(
            check_can_remove(ConversationUserRole::Member, ConversationUserRole::Member).is_err()
        );
        assert!(
            check_can_remove(ConversationUserRole::Member, ConversationUserRole::Admin).is_err()
        );
    }
}
