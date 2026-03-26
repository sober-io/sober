//! Background subscription to the agent's conversation update stream.
//!
//! Spawns a long-lived task that calls `SubscribeConversationUpdates` on the
//! agent gRPC service and routes incoming events to the correct WebSocket
//! connections via the [`ConnectionRegistry`].

use futures::StreamExt;
use sober_core::types::{ConversationId, ConversationUserRepo, UserId};
use sober_db::PgConversationUserRepo;
use sqlx::PgPool;
use tracing::{error, info, warn};

use crate::connections::{ConnectionRegistry, UserConnectionRegistry};
use crate::proto;
use crate::routes::ws::ServerWsMessage;
use crate::state::AgentClient;

/// Spawns the subscription background task.
///
/// This task runs forever, reconnecting with exponential backoff if the
/// gRPC stream breaks. Events are routed to WebSocket clients via the
/// [`ConnectionRegistry`]. Unread notifications are sent via the
/// [`UserConnectionRegistry`].
pub fn spawn_subscription(
    agent_client: AgentClient,
    registry: ConnectionRegistry,
    user_connections: UserConnectionRegistry,
    db: PgPool,
) {
    tokio::spawn(async move {
        subscription_loop(agent_client, registry, user_connections, db).await;
    });
}

/// Backoff delays for reconnection attempts (in seconds).
const BACKOFF_DELAYS: &[u64] = &[1, 2, 5, 10, 30];

/// Reconnection loop that subscribes to the agent and processes events.
async fn subscription_loop(
    mut agent_client: AgentClient,
    registry: ConnectionRegistry,
    user_connections: UserConnectionRegistry,
    db: PgPool,
) {
    let mut attempt = 0usize;

    loop {
        info!("subscribing to agent conversation updates");

        match agent_client
            .subscribe_conversation_updates(proto::SubscribeRequest {})
            .await
        {
            Ok(response) => {
                attempt = 0;
                let mut stream = response.into_inner();

                while let Some(result) = stream.next().await {
                    match result {
                        Ok(update) => {
                            let conversation_id = update.conversation_id.clone();

                            // Handle unread notifications for NewMessage events.
                            if let Some(proto::conversation_update::Event::NewMessage(ref nm)) =
                                update.event
                            {
                                handle_new_message_unread(
                                    &conversation_id,
                                    nm,
                                    &db,
                                    &user_connections,
                                )
                                .await;
                            }

                            if let Some(ws_msg) = conversation_update_to_ws(update) {
                                registry.send(&conversation_id, ws_msg).await;
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "conversation update stream error");
                            break;
                        }
                    }
                }

                info!("conversation update stream ended, reconnecting");
            }
            Err(e) => {
                error!(error = %e, "failed to subscribe to conversation updates");
            }
        }

        let delay = BACKOFF_DELAYS[attempt.min(BACKOFF_DELAYS.len() - 1)];
        attempt += 1;
        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
    }
}

/// Increments unread counts for all users in a conversation except the sender,
/// and sends `chat.unread` notifications via the user connection registry.
async fn handle_new_message_unread(
    conversation_id: &str,
    nm: &proto::NewMessage,
    db: &PgPool,
    user_connections: &UserConnectionRegistry,
) {
    let Ok(conv_uuid) = conversation_id.parse::<uuid::Uuid>() else {
        return;
    };
    let conv_id = ConversationId::from_uuid(conv_uuid);

    // Determine the sender user to exclude from unread increment.
    // If no user_id is present (system/scheduler messages), use a nil UUID
    // so all users get their unread count incremented.
    let exclude_user_id = nm
        .user_id
        .as_deref()
        .and_then(|id| id.parse::<uuid::Uuid>().ok())
        .map(UserId::from_uuid)
        .unwrap_or_default();

    let cu_repo = PgConversationUserRepo::new(db.clone());
    if let Ok(affected) = cu_repo.increment_unread(conv_id, exclude_user_id).await {
        for (user_id, new_count) in affected {
            user_connections
                .send(
                    &user_id.to_string(),
                    ServerWsMessage::ChatUnread {
                        conversation_id: conversation_id.to_string(),
                        unread_count: new_count,
                    },
                )
                .await;
        }
    }
}

/// Converts a proto `ConversationUpdate` into a `ServerWsMessage` for
/// forwarding to the WebSocket client.
fn conversation_update_to_ws(update: proto::ConversationUpdate) -> Option<ServerWsMessage> {
    let cid = update.conversation_id;

    match update.event? {
        proto::conversation_update::Event::TextDelta(td) => Some(ServerWsMessage::ChatDelta {
            conversation_id: cid,
            content: td.content,
        }),
        proto::conversation_update::Event::ToolExecutionUpdate(teu) => {
            Some(ServerWsMessage::ChatToolExecutionUpdate {
                conversation_id: cid,
                id: teu.id,
                message_id: teu.message_id,
                tool_call_id: teu.tool_call_id,
                tool_name: teu.tool_name,
                status: teu.status,
                output: teu.output,
                error: teu.error,
                input: teu.input,
            })
        }
        proto::conversation_update::Event::Done(done) => Some(ServerWsMessage::ChatDone {
            conversation_id: cid,
            message_id: done.message_id,
        }),
        proto::conversation_update::Event::ThinkingDelta(td) => {
            Some(ServerWsMessage::ChatThinking {
                conversation_id: cid,
                content: td.content,
            })
        }
        proto::conversation_update::Event::TitleChanged(tc) => Some(ServerWsMessage::ChatTitle {
            conversation_id: cid,
            title: tc.title,
        }),
        proto::conversation_update::Event::Error(e) => Some(ServerWsMessage::ChatError {
            conversation_id: cid,
            error: e.message,
        }),
        proto::conversation_update::Event::ConfirmRequest(cr) => {
            Some(ServerWsMessage::ChatConfirm {
                conversation_id: cid,
                confirm_id: cr.confirm_id,
                command: cr.command,
                risk_level: cr.risk_level,
                affects: cr.affects,
                reason: cr.reason,
            })
        }
        proto::conversation_update::Event::NewMessage(nm) => {
            let source = serde_json::from_value::<sober_core::types::access::TriggerKind>(
                serde_json::Value::String(nm.source),
            )
            .unwrap_or(sober_core::types::access::TriggerKind::Human);
            Some(ServerWsMessage::ChatNewMessage {
                conversation_id: cid,
                message_id: nm.message_id,
                role: nm.role,
                content: nm.content,
                source,
                user_id: nm.user_id.filter(|s| !s.is_empty()),
                username: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_text_delta() {
        let update = proto::ConversationUpdate {
            conversation_id: "conv-1".to_owned(),
            event: Some(proto::conversation_update::Event::TextDelta(
                proto::TextDelta {
                    content: "hello".to_owned(),
                },
            )),
        };
        let ws = conversation_update_to_ws(update).expect("should convert");
        match ws {
            ServerWsMessage::ChatDelta {
                conversation_id,
                content,
            } => {
                assert_eq!(conversation_id, "conv-1");
                assert_eq!(content, "hello");
            }
            _ => panic!("unexpected message type"),
        }
    }

    #[test]
    fn convert_new_message() {
        let update = proto::ConversationUpdate {
            conversation_id: "conv-1".to_owned(),
            event: Some(proto::conversation_update::Event::NewMessage(
                proto::NewMessage {
                    message_id: "msg-1".to_owned(),
                    role: "Assistant".to_owned(),
                    content: "hi".to_owned(),
                    source: "scheduler".to_owned(),
                    user_id: None,
                },
            )),
        };
        let ws = conversation_update_to_ws(update).expect("should convert");
        match ws {
            ServerWsMessage::ChatNewMessage {
                conversation_id,
                message_id,
                role,
                content,
                source,
                user_id: _,
                username: _,
            } => {
                assert_eq!(conversation_id, "conv-1");
                assert_eq!(message_id, "msg-1");
                assert_eq!(role, "Assistant");
                assert_eq!(content, "hi");
                assert_eq!(source, sober_core::types::access::TriggerKind::Scheduler);
            }
            _ => panic!("unexpected message type"),
        }
    }

    #[test]
    fn convert_none_event() {
        let update = proto::ConversationUpdate {
            conversation_id: "conv-1".to_owned(),
            event: None,
        };
        assert!(conversation_update_to_ws(update).is_none());
    }
}
