//! Background subscription to the agent's conversation update stream.
//!
//! Spawns a long-lived task that calls `SubscribeConversationUpdates` on the
//! agent gRPC service and routes incoming events to the correct WebSocket
//! connections via the [`ConnectionRegistry`].

use futures::StreamExt;
use tracing::{error, info, warn};

use crate::connections::ConnectionRegistry;
use crate::proto;
use crate::routes::ws::ServerWsMessage;
use crate::state::AgentClient;

/// Spawns the subscription background task.
///
/// This task runs forever, reconnecting with exponential backoff if the
/// gRPC stream breaks. Events are routed to WebSocket clients via the
/// [`ConnectionRegistry`].
pub fn spawn_subscription(agent_client: AgentClient, registry: ConnectionRegistry) {
    tokio::spawn(async move {
        subscription_loop(agent_client, registry).await;
    });
}

/// Backoff delays for reconnection attempts (in seconds).
const BACKOFF_DELAYS: &[u64] = &[1, 2, 5, 10, 30];

/// Reconnection loop that subscribes to the agent and processes events.
async fn subscription_loop(mut agent_client: AgentClient, registry: ConnectionRegistry) {
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

/// Converts a proto `ConversationUpdate` into a `ServerWsMessage` for
/// forwarding to the WebSocket client.
fn conversation_update_to_ws(update: proto::ConversationUpdate) -> Option<ServerWsMessage> {
    let cid = update.conversation_id;

    match update.event? {
        proto::conversation_update::Event::TextDelta(td) => Some(ServerWsMessage::ChatDelta {
            conversation_id: cid,
            content: td.content,
        }),
        proto::conversation_update::Event::ToolCallStart(tcs) => {
            Some(ServerWsMessage::ChatToolUse {
                conversation_id: cid,
                tool_call: serde_json::json!({
                    "name": tcs.name,
                    "input": tcs.input_json,
                }),
            })
        }
        proto::conversation_update::Event::ToolCallResult(tcr) => {
            Some(ServerWsMessage::ChatToolResult {
                conversation_id: cid,
                tool_call_id: tcr.name.clone(),
                output: tcr.output,
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
            Some(ServerWsMessage::ChatNewMessage {
                conversation_id: cid,
                message_id: nm.message_id,
                role: nm.role,
                content: nm.content,
                source: nm.source,
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
            } => {
                assert_eq!(conversation_id, "conv-1");
                assert_eq!(message_id, "msg-1");
                assert_eq!(role, "Assistant");
                assert_eq!(content, "hi");
                assert_eq!(source, "scheduler");
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
