use sober_core::error::AppError;
use sober_core::types::{ContentBlock, ConversationId, ConversationUserRepo, MessageRepo, UserId};
use sober_db::{PgConversationUserRepo, PgMessageRepo};
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{error, warn};

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

    /// Process a chat message: verify membership, broadcast, and fire-and-forget gRPC.
    pub async fn send_message(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        username: &str,
        content: Vec<ContentBlock>,
        error_tx: mpsc::Sender<ServerWsMessage>,
    ) -> Result<(), AppError> {
        crate::services::verify_membership(&self.db, conversation_id, user_id).await?;

        let conv_id_str = conversation_id.to_string();

        // Broadcast the user's message to all subscribers.
        let user_msg = ServerWsMessage::ChatNewMessage {
            conversation_id: conv_id_str.clone(),
            message_id: uuid::Uuid::now_v7().to_string(),
            role: "user".into(),
            content: content.clone(),
            source: sober_core::types::access::TriggerKind::Human,
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

        // Convert content blocks to proto format.
        let proto_blocks: Vec<proto::ContentBlock> =
            content.into_iter().map(content_block_to_proto).collect();

        // Call unary HandleMessage RPC — fire and forget.
        let mut agent_client = self.agent_client.clone();
        let mut request = tonic::Request::new(proto::HandleMessageRequest {
            user_id: user_id.to_string(),
            conversation_id: conv_id_str.clone(),
            content: proto_blocks,
        });

        let span = tracing::info_span!(
            "ws.handle_message",
            otel.kind = "client",
            rpc.service = "AgentService",
            rpc.method = "HandleMessage",
            rpc.system = "grpc",
            user.id = %user_id,
            conversation.id = %conv_id_str,
            otel.status_code = tracing::field::Empty,
        );
        {
            use tracing_opentelemetry::OpenTelemetrySpanExt;
            let cx = span.context();
            opentelemetry::global::get_text_map_propagator(|p| {
                p.inject_context(
                    &cx,
                    &mut sober_core::MetadataMapInjector(request.metadata_mut()),
                );
            });
        }
        tokio::spawn(tracing::Instrument::instrument(
            async move {
                match agent_client.handle_message(request).await {
                    Ok(_) => {
                        tracing::Span::current().record("otel.status_code", "OK");
                    }
                    Err(e) => {
                        tracing::Span::current().record("otel.status_code", "ERROR");
                        error!(
                            error.message = %e.message(),
                            error.type_ = %e.code(),
                            "HandleMessage RPC failed"
                        );
                        let _ = error_tx
                            .send(ServerWsMessage::ChatError {
                                conversation_id: conv_id_str,
                                error: e.message().to_owned(),
                            })
                            .await;
                    }
                }
            },
            span,
        ));

        Ok(())
    }

    /// Submit a confirmation response.
    pub async fn confirm_response(
        &self,
        confirm_id: String,
        approved: bool,
    ) -> Result<(), AppError> {
        let mut agent_client = self.agent_client.clone();
        let resp = proto::ConfirmResponse {
            confirm_id,
            approved,
        };
        if let Err(e) = agent_client.submit_confirmation(resp).await {
            warn!(error = %e, "failed to submit confirmation");
        }
        Ok(())
    }

    /// Set the permission mode on the agent.
    pub async fn set_permission_mode(&self, mode: String) -> Result<(), AppError> {
        let mut agent_client = self.agent_client.clone();
        let req = proto::SetPermissionModeRequest { mode: mode.clone() };
        if let Err(e) = agent_client.set_permission_mode(req).await {
            warn!(error = %e, mode, "failed to set permission mode");
        }
        Ok(())
    }
}

fn content_block_to_proto(block: ContentBlock) -> proto::ContentBlock {
    match block {
        ContentBlock::Text { text } => proto::ContentBlock {
            block: Some(proto::content_block::Block::Text(proto::TextBlock { text })),
        },
        ContentBlock::Image {
            conversation_attachment_id,
            alt,
        } => proto::ContentBlock {
            block: Some(proto::content_block::Block::Image(proto::ImageBlock {
                conversation_attachment_id: conversation_attachment_id.to_string(),
                alt,
            })),
        },
        ContentBlock::File {
            conversation_attachment_id,
        } => proto::ContentBlock {
            block: Some(proto::content_block::Block::File(proto::FileBlock {
                conversation_attachment_id: conversation_attachment_id.to_string(),
            })),
        },
        ContentBlock::Audio {
            conversation_attachment_id,
        } => proto::ContentBlock {
            block: Some(proto::content_block::Block::Audio(proto::AudioBlock {
                conversation_attachment_id: conversation_attachment_id.to_string(),
            })),
        },
        ContentBlock::Video {
            conversation_attachment_id,
        } => proto::ContentBlock {
            block: Some(proto::content_block::Block::Video(proto::VideoBlock {
                conversation_attachment_id: conversation_attachment_id.to_string(),
            })),
        },
    }
}
