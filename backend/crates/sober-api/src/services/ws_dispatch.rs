use sober_core::error::AppError;
use sober_core::types::{
    ContentBlock, ConversationId, ConversationUserRepo, MessageRepo, MessageSource, UserId,
};
use sober_db::{PgConversationUserRepo, PgMessageRepo};
use sqlx::PgPool;
use tracing::{error, instrument, warn};

use crate::connections::ConnectionRegistry;
use crate::proto;
use crate::state::AgentClient;
use crate::ws_types::ServerWsMessage;

pub struct WsDispatchService {
    db: PgPool,
    agent_client: AgentClient,
    connections: ConnectionRegistry,
}

impl WsDispatchService {
    pub fn new(db: PgPool, agent_client: AgentClient, connections: ConnectionRegistry) -> Self {
        Self {
            db,
            agent_client,
            connections,
        }
    }

    /// Verify membership and mark conversation as read (best-effort).
    #[instrument(skip(self), fields(conversation.id = %conversation_id))]
    pub async fn subscribe(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> Result<(), AppError> {
        crate::services::verify_membership(&self.db, conversation_id, user_id).await?;

        // Mark as read (best-effort).
        let cu_repo = PgConversationUserRepo::new(self.db.clone());
        let msg_repo = PgMessageRepo::new(self.db.clone());
        if let Ok(messages) = msg_repo.list_paginated(conversation_id, None, 1).await
            && let Some(latest) = messages.first()
        {
            cu_repo
                .mark_read(conversation_id, user_id, latest.id)
                .await
                .ok();
        }

        Ok(())
    }

    /// Process a chat message: verify membership, store via gRPC, then broadcast.
    #[instrument(skip(self, content), fields(conversation.id = %conversation_id))]
    pub async fn send_message(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        username: &str,
        content: Vec<ContentBlock>,
    ) -> Result<(), AppError> {
        crate::services::verify_membership(&self.db, conversation_id, user_id).await?;

        let conv_id_str = conversation_id.to_string();

        // Convert content blocks to proto format.
        let proto_blocks: Vec<proto::ContentBlock> = content
            .iter()
            .cloned()
            .map(crate::proto_convert::content_block_to_proto)
            .collect();

        // Call unary HandleMessage RPC — returns immediately with the stored
        // message ID. The agent processes the turn asynchronously.
        let mut agent_client = self.agent_client.clone();
        let mut request = tonic::Request::new(proto::HandleMessageRequest {
            user_id: user_id.to_string(),
            conversation_id: conv_id_str.clone(),
            content: proto_blocks,
            source: proto::MessageSource::Web.into(),
        });
        sober_core::inject_trace_context(request.metadata_mut());

        let response = agent_client.handle_message(request).await.map_err(|e| {
            error!(
                error.message = %e.message(),
                error.type_ = %e.code(),
                "HandleMessage RPC failed"
            );
            AppError::Internal(anyhow::anyhow!("agent error: {}", e.message()).into())
        })?;
        let message_id = response.into_inner().message_id;

        // Broadcast the user's message with the real DB ID to all subscribers.
        let user_msg = ServerWsMessage::ChatNewMessage {
            conversation_id: conv_id_str.clone(),
            message_id,
            role: "user".into(),
            content,
            source: MessageSource::Web,
            user_id: Some(user_id.to_string()),
            username: Some(username.to_string()),
        };
        self.connections.send(&conv_id_str, user_msg).await;

        // Notify subscribers that the agent is processing.
        self.connections
            .send(
                &conv_id_str,
                ServerWsMessage::ChatAgentTyping {
                    conversation_id: conv_id_str.clone(),
                },
            )
            .await;

        Ok(())
    }

    /// Submit a confirmation response.
    #[instrument(skip(self))]
    pub async fn confirm_response(
        &self,
        confirm_id: String,
        approved: bool,
    ) -> Result<(), AppError> {
        let mut agent_client = self.agent_client.clone();
        let mut request = tonic::Request::new(proto::ConfirmResponse {
            confirm_id,
            approved,
        });
        sober_core::inject_trace_context(request.metadata_mut());
        if let Err(e) = agent_client.submit_confirmation(request).await {
            warn!(error = %e, "failed to submit confirmation");
        }
        Ok(())
    }

    /// Set the permission mode on the agent.
    #[instrument(skip(self))]
    pub async fn set_permission_mode(&self, mode: String) -> Result<(), AppError> {
        let mut agent_client = self.agent_client.clone();
        let mut request =
            tonic::Request::new(proto::SetPermissionModeRequest { mode: mode.clone() });
        sober_core::inject_trace_context(request.metadata_mut());
        if let Err(e) = agent_client.set_permission_mode(request).await {
            warn!(error = %e, mode, "failed to set permission mode");
        }
        Ok(())
    }
}
