use serde::Serialize;
use sober_core::config::AppConfig;
use sober_core::error::AppError;
use sober_core::types::{
    AgentMode, ConversationId, ConversationKind, ConversationRepo, ConversationUserRepo,
    ConversationUserRole, ConversationWithDetails, Job, JobRepo, ListConversationsFilter,
    MessageId, MessageRepo, PermissionMode, PluginId, SandboxNetMode, TagRepo, WorkspaceRepo,
    WorkspaceSettingsRepo,
};
use sober_db::{
    PgConversationRepo, PgConversationUserRepo, PgJobRepo, PgMessageRepo, PgTagRepo,
    PgWorkspaceRepo, PgWorkspaceSettingsRepo,
};
use sqlx::PgPool;

/// Maximum length for auto-generated workspace names (truncated from conversation title).
const MAX_WORKSPACE_NAME_LEN: usize = 80;

/// Response for conversation creation.
#[derive(Serialize)]
pub struct CreateConversationResponse {
    pub id: String,
    pub title: Option<String>,
    pub workspace_id: Option<String>,
    pub kind: ConversationKind,
    pub agent_mode: AgentMode,
    pub is_archived: bool,
    pub unread_count: i32,
    pub last_read_message_id: Option<String>,
    pub tags: Vec<sober_core::types::Tag>,
    pub created_at: String,
    pub updated_at: String,
}

/// Response for conversation update.
#[derive(Serialize)]
pub struct UpdateConversationResponse {
    pub id: String,
    pub title: Option<String>,
    pub kind: ConversationKind,
    pub agent_mode: AgentMode,
    pub is_archived: bool,
}

/// Response for inbox.
#[derive(Serialize)]
pub struct InboxResponse {
    pub id: String,
    pub title: Option<String>,
    pub kind: ConversationKind,
    pub is_archived: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Response for convert-to-group.
#[derive(Serialize)]
pub struct ConvertToGroupResponse {
    pub id: String,
    pub title: Option<String>,
    pub kind: ConversationKind,
    pub agent_mode: AgentMode,
    pub is_archived: bool,
}

/// Combined response for conversation settings.
#[derive(Serialize)]
pub struct SettingsResponse {
    pub permission_mode: PermissionMode,
    pub agent_mode: AgentMode,
    pub sandbox_profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_net_mode: Option<SandboxNetMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_allowed_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_max_execution_seconds: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_allow_spawn: Option<bool>,
    pub auto_snapshot: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_snapshots: Option<i32>,
    pub disabled_tools: Vec<String>,
    pub disabled_plugins: Vec<String>,
}

/// Typed input for settings updates (handler deserializes, service takes typed input).
pub struct UpdateSettingsInput {
    pub permission_mode: Option<PermissionMode>,
    pub agent_mode: Option<AgentMode>,
    pub sandbox_profile: Option<String>,
    pub sandbox_net_mode: Option<SandboxNetMode>,
    pub sandbox_allowed_domains: Option<Vec<String>>,
    pub sandbox_max_execution_seconds: Option<i32>,
    pub sandbox_allow_spawn: Option<bool>,
    pub auto_snapshot: Option<bool>,
    pub max_snapshots: Option<i32>,
    pub disabled_tools: Option<Vec<String>>,
    pub disabled_plugins: Option<Vec<String>>,
}

pub struct ConversationService {
    db: PgPool,
    config: AppConfig,
}

impl ConversationService {
    pub fn new(db: PgPool, config: AppConfig) -> Self {
        Self { db, config }
    }

    /// List conversations with details for a user.
    pub async fn list(
        &self,
        user_id: sober_core::types::UserId,
        filter: ListConversationsFilter,
    ) -> Result<Vec<ConversationWithDetails>, AppError> {
        let repo = PgConversationRepo::new(self.db.clone());
        repo.list_with_details(user_id, filter).await
    }

    /// Create a new direct conversation with workspace provisioning.
    pub async fn create(
        &self,
        user_id: sober_core::types::UserId,
        title: Option<&str>,
    ) -> Result<CreateConversationResponse, AppError> {
        let ws_name = title
            .unwrap_or("untitled")
            .chars()
            .take(MAX_WORKSPACE_NAME_LEN)
            .collect::<String>();
        let ws_root = format!(
            "{}/{}",
            self.config.workspace_root.display(),
            uuid::Uuid::now_v7()
        );

        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let (workspace, _settings) =
            PgWorkspaceRepo::provision_tx(&mut tx, user_id, &ws_name, &ws_root).await?;
        let conversation =
            PgConversationRepo::create_tx(&mut tx, user_id, title, Some(workspace.id)).await?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(CreateConversationResponse {
            id: conversation.id.to_string(),
            title: conversation.title,
            workspace_id: conversation.workspace_id.map(|w| w.to_string()),
            kind: conversation.kind,
            agent_mode: conversation.agent_mode,
            is_archived: conversation.is_archived,
            unread_count: 0,
            last_read_message_id: None,
            tags: vec![],
            created_at: conversation.created_at.to_rfc3339(),
            updated_at: conversation.updated_at.to_rfc3339(),
        })
    }

    /// Get a conversation with full details.
    pub async fn get(
        &self,
        conversation_id: ConversationId,
        user_id: sober_core::types::UserId,
    ) -> Result<ConversationWithDetails, AppError> {
        let cu = super::verify_membership(&self.db, conversation_id, user_id).await?;

        let conv_repo = PgConversationRepo::new(self.db.clone());
        let cu_repo = PgConversationUserRepo::new(self.db.clone());
        let tag_repo = PgTagRepo::new(self.db.clone());

        let conversation = conv_repo.get_by_id(conversation_id).await?;
        let users = cu_repo.list_by_conversation(conversation_id).await?;
        let tags = tag_repo.list_by_conversation(conversation_id).await?;

        let (workspace_name, workspace_path) = if let Some(ws_id) = conversation.workspace_id {
            let ws_repo = PgWorkspaceRepo::new(self.db.clone());
            match ws_repo.get_by_id(ws_id).await {
                Ok(ws) => (Some(ws.name), Some(ws.root_path)),
                Err(_) => (None, None),
            }
        } else {
            (None, None)
        };

        Ok(ConversationWithDetails {
            conversation,
            unread_count: cu.unread_count,
            last_read_message_id: cu.last_read_message_id,
            tags,
            users,
            workspace_name,
            workspace_path,
        })
    }

    /// Update conversation title and/or archived status.
    pub async fn update(
        &self,
        conversation_id: ConversationId,
        user_id: sober_core::types::UserId,
        title: Option<&str>,
        archived: Option<bool>,
    ) -> Result<UpdateConversationResponse, AppError> {
        let repo = PgConversationRepo::new(self.db.clone());

        super::verify_membership(&self.db, conversation_id, user_id).await?;

        if let Some(title) = title {
            repo.update_title(conversation_id, title).await?;
        }
        if let Some(archived) = archived {
            repo.update_archived(conversation_id, archived).await?;
        }

        let updated = repo.get_by_id(conversation_id).await?;

        Ok(UpdateConversationResponse {
            id: updated.id.to_string(),
            title: updated.title,
            kind: updated.kind,
            agent_mode: updated.agent_mode,
            is_archived: updated.is_archived,
        })
    }

    /// Delete a conversation (owner only).
    pub async fn delete(
        &self,
        conversation_id: ConversationId,
        user_id: sober_core::types::UserId,
    ) -> Result<(), AppError> {
        let membership = super::verify_membership(&self.db, conversation_id, user_id).await?;
        if membership.role != ConversationUserRole::Owner {
            return Err(AppError::Forbidden);
        }

        PgConversationRepo::new(self.db.clone())
            .delete(conversation_id)
            .await
    }

    /// Get combined settings for a conversation.
    pub async fn get_settings(
        &self,
        conversation_id: ConversationId,
        user_id: sober_core::types::UserId,
    ) -> Result<SettingsResponse, AppError> {
        super::verify_membership(&self.db, conversation_id, user_id).await?;

        let conv_repo = PgConversationRepo::new(self.db.clone());
        let ws_settings_repo = PgWorkspaceSettingsRepo::new(self.db.clone());

        let conversation = conv_repo.get_by_id(conversation_id).await?;
        let ws_id = conversation
            .workspace_id
            .ok_or_else(|| AppError::NotFound("workspace_settings".into()))?;
        let settings = ws_settings_repo.get_by_workspace(ws_id).await?;

        Ok(settings_to_response(&settings, conversation.agent_mode))
    }

    /// Partial update of conversation settings.
    pub async fn update_settings(
        &self,
        conversation_id: ConversationId,
        user_id: sober_core::types::UserId,
        input: UpdateSettingsInput,
    ) -> Result<SettingsResponse, AppError> {
        super::verify_membership(&self.db, conversation_id, user_id).await?;

        let conv_repo = PgConversationRepo::new(self.db.clone());
        let ws_settings_repo = PgWorkspaceSettingsRepo::new(self.db.clone());

        let conversation = conv_repo.get_by_id(conversation_id).await?;
        let ws_id = conversation
            .workspace_id
            .ok_or_else(|| AppError::NotFound("workspace_settings".into()))?;

        let mut settings = ws_settings_repo.get_by_workspace(ws_id).await?;

        if let Some(mode) = input.permission_mode {
            settings.permission_mode = mode;
        }
        if let Some(profile) = input.sandbox_profile {
            settings.sandbox_profile = profile;
        }
        if let Some(net_mode) = input.sandbox_net_mode {
            settings.sandbox_net_mode = Some(net_mode);
        }
        if let Some(domains) = input.sandbox_allowed_domains {
            settings.sandbox_allowed_domains = Some(domains);
        }
        if let Some(seconds) = input.sandbox_max_execution_seconds {
            settings.sandbox_max_execution_seconds = Some(seconds);
        }
        if let Some(spawn) = input.sandbox_allow_spawn {
            settings.sandbox_allow_spawn = Some(spawn);
        }
        if let Some(snap) = input.auto_snapshot {
            settings.auto_snapshot = snap;
        }
        if let Some(max) = input.max_snapshots {
            settings.max_snapshots = Some(max);
        }
        if let Some(tools) = input.disabled_tools {
            settings.disabled_tools = tools;
        }
        if let Some(plugins) = input.disabled_plugins {
            settings.disabled_plugins = plugins
                .into_iter()
                .filter_map(|s| uuid::Uuid::parse_str(&s).ok().map(PluginId::from_uuid))
                .collect();
        }

        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if let Some(agent_mode) = input.agent_mode {
            PgConversationRepo::update_agent_mode_tx(&mut tx, conversation_id, agent_mode).await?;
        }

        let updated = PgWorkspaceSettingsRepo::upsert_tx(&mut tx, &settings).await?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let conv = conv_repo.get_by_id(conversation_id).await?;

        Ok(settings_to_response(&updated, conv.agent_mode))
    }

    /// Mark a conversation as read.
    pub async fn mark_read(
        &self,
        conversation_id: ConversationId,
        user_id: sober_core::types::UserId,
        message_id: Option<MessageId>,
    ) -> Result<(), AppError> {
        super::verify_membership(&self.db, conversation_id, user_id).await?;

        let message_id = match message_id {
            Some(mid) => mid,
            None => {
                let msg_repo = PgMessageRepo::new(self.db.clone());
                let messages = msg_repo.list_paginated(conversation_id, None, 1).await?;
                match messages.first() {
                    Some(msg) => msg.id,
                    None => return Ok(()),
                }
            }
        };

        let cu_repo = PgConversationUserRepo::new(self.db.clone());
        cu_repo
            .mark_read(conversation_id, user_id, message_id)
            .await?;

        Ok(())
    }

    /// Get the user's inbox conversation.
    pub async fn get_inbox(
        &self,
        user_id: sober_core::types::UserId,
    ) -> Result<InboxResponse, AppError> {
        let repo = PgConversationRepo::new(self.db.clone());
        let conv = repo.get_inbox(user_id).await?;

        Ok(InboxResponse {
            id: conv.id.to_string(),
            title: conv.title,
            kind: conv.kind,
            is_archived: conv.is_archived,
            created_at: conv.created_at.to_rfc3339(),
            updated_at: conv.updated_at.to_rfc3339(),
        })
    }

    /// Convert a direct conversation to a group (owner only).
    pub async fn convert_to_group(
        &self,
        conversation_id: ConversationId,
        user_id: sober_core::types::UserId,
        title: &str,
    ) -> Result<ConvertToGroupResponse, AppError> {
        let conv_repo = PgConversationRepo::new(self.db.clone());

        let membership = super::verify_membership(&self.db, conversation_id, user_id).await?;
        if membership.role != ConversationUserRole::Owner {
            return Err(AppError::Forbidden);
        }

        let conversation = conv_repo.get_by_id(conversation_id).await?;
        if conversation.kind != ConversationKind::Direct {
            return Err(AppError::Validation(
                "only direct conversations can be converted to group".into(),
            ));
        }

        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        PgConversationRepo::convert_to_group_tx(&mut tx, conversation_id).await?;
        PgConversationRepo::update_title_tx(&mut tx, conversation_id, title).await?;
        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let updated = conv_repo.get_by_id(conversation_id).await?;

        Ok(ConvertToGroupResponse {
            id: updated.id.to_string(),
            title: updated.title,
            kind: updated.kind,
            agent_mode: updated.agent_mode,
            is_archived: updated.is_archived,
        })
    }

    /// Clear all messages in a conversation (owner only).
    pub async fn clear_messages(
        &self,
        conversation_id: ConversationId,
        user_id: sober_core::types::UserId,
    ) -> Result<(), AppError> {
        let membership = super::verify_membership(&self.db, conversation_id, user_id).await?;
        if membership.role != ConversationUserRole::Owner {
            return Err(AppError::Forbidden);
        }

        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        PgMessageRepo::clear_conversation_tx(&mut tx, conversation_id).await?;
        PgConversationUserRepo::reset_all_unread_tx(&mut tx, conversation_id).await?;
        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    /// List jobs linked to a conversation.
    pub async fn list_jobs(
        &self,
        conversation_id: ConversationId,
        user_id: sober_core::types::UserId,
    ) -> Result<Vec<Job>, AppError> {
        super::verify_membership(&self.db, conversation_id, user_id).await?;

        let job_repo = PgJobRepo::new(self.db.clone());
        job_repo
            .list_filtered(
                None,
                None,
                &[],
                None,
                None,
                Some(*conversation_id.as_uuid()),
            )
            .await
    }
}

fn settings_to_response(
    settings: &sober_core::types::WorkspaceSettings,
    agent_mode: AgentMode,
) -> SettingsResponse {
    SettingsResponse {
        permission_mode: settings.permission_mode,
        agent_mode,
        sandbox_profile: settings.sandbox_profile.clone(),
        sandbox_net_mode: settings.sandbox_net_mode,
        sandbox_allowed_domains: settings.sandbox_allowed_domains.clone(),
        sandbox_max_execution_seconds: settings.sandbox_max_execution_seconds,
        sandbox_allow_spawn: settings.sandbox_allow_spawn,
        auto_snapshot: settings.auto_snapshot,
        max_snapshots: settings.max_snapshots,
        disabled_tools: settings.disabled_tools.clone(),
        disabled_plugins: settings
            .disabled_plugins
            .iter()
            .map(|id| id.to_string())
            .collect(),
    }
}
