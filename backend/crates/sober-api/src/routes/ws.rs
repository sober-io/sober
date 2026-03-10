//! WebSocket chat handler.
//!
//! Single endpoint at `/api/v1/ws`. All messages include `conversation_id`
//! in the payload for multiplexing across conversations on one connection.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use futures::{SinkExt, StreamExt};
use sober_auth::AuthUser;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::proto;
use crate::state::{AgentClient, AppState};

/// Returns the WebSocket route.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/ws", get(ws_upgrade))
}

/// `GET /api/v1/ws` — upgrade to WebSocket.
async fn ws_upgrade(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, auth_user))
}

/// Client-to-server WebSocket message types.
#[derive(serde::Deserialize)]
#[serde(tag = "type")]
enum ClientWsMessage {
    #[serde(rename = "chat.message")]
    ChatMessage {
        conversation_id: String,
        content: String,
    },
    #[serde(rename = "chat.cancel")]
    ChatCancel { conversation_id: String },
}

/// Server-to-client WebSocket message types.
#[derive(serde::Serialize)]
#[serde(tag = "type")]
#[expect(clippy::enum_variant_names)]
enum ServerWsMessage {
    #[serde(rename = "chat.delta")]
    ChatDelta {
        conversation_id: String,
        content: String,
    },
    #[serde(rename = "chat.tool_use")]
    ChatToolUse {
        conversation_id: String,
        tool_call: serde_json::Value,
    },
    #[serde(rename = "chat.tool_result")]
    ChatToolResult {
        conversation_id: String,
        tool_call_id: String,
        output: String,
    },
    #[serde(rename = "chat.done")]
    ChatDone {
        conversation_id: String,
        message_id: String,
    },
    #[serde(rename = "chat.thinking")]
    ChatThinking {
        conversation_id: String,
        content: String,
    },
    #[serde(rename = "chat.title")]
    ChatTitle {
        conversation_id: String,
        title: String,
    },
    #[serde(rename = "chat.error")]
    ChatError {
        conversation_id: String,
        error: String,
    },
}

/// Handles a single WebSocket connection.
async fn handle_socket(socket: WebSocket, state: Arc<AppState>, auth_user: AuthUser) {
    let user_id = auth_user.user_id;
    info!(user_id = %user_id, "WebSocket connected");

    let (mut ws_tx, mut ws_rx) = socket.split();

    // Channel for sending messages back to the client from spawned tasks.
    let (out_tx, mut out_rx) = mpsc::channel::<ServerWsMessage>(64);

    // Track active conversation tasks for cancellation.
    let mut active_tasks: HashMap<String, CancellationToken> = HashMap::new();

    // Spawn a task that forwards outbound messages to the WebSocket.
    let send_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            match serde_json::to_string(&msg) {
                Ok(text) => {
                    if ws_tx.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    error!(error = %e, "failed to serialize WebSocket message");
                }
            }
        }
    });

    // Process incoming messages.
    while let Some(msg) = ws_rx.next().await {
        let msg = match msg {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => break,
            Ok(_) => continue, // Ignore binary, ping, pong.
            Err(e) => {
                warn!(error = %e, "WebSocket receive error");
                break;
            }
        };

        let client_msg: ClientWsMessage = match serde_json::from_str(&msg) {
            Ok(m) => m,
            Err(e) => {
                let error_msg = ServerWsMessage::ChatError {
                    conversation_id: String::new(),
                    error: format!("invalid message format: {e}"),
                };
                let _ = out_tx.send(error_msg).await;
                continue;
            }
        };

        match client_msg {
            ClientWsMessage::ChatMessage {
                conversation_id,
                content,
            } => {
                let cancel_token = CancellationToken::new();
                active_tasks.insert(conversation_id.clone(), cancel_token.clone());

                let out_tx = out_tx.clone();
                let agent_client = state.agent_client.clone();
                let conv_id = conversation_id.clone();
                let uid = user_id.to_string();

                tokio::spawn(async move {
                    handle_chat_message(agent_client, uid, conv_id, content, out_tx, cancel_token)
                        .await;
                });
            }
            ClientWsMessage::ChatCancel { conversation_id } => {
                if let Some(token) = active_tasks.remove(&conversation_id) {
                    token.cancel();
                    info!(conversation_id, "cancelled active chat task");
                }
            }
        }
    }

    // Cancel all active tasks on disconnect.
    for token in active_tasks.values() {
        token.cancel();
    }

    send_task.abort();
    info!(user_id = %user_id, "WebSocket disconnected");
}

/// Handles a single chat message by calling the agent via gRPC streaming.
async fn handle_chat_message(
    mut agent_client: AgentClient,
    user_id: String,
    conversation_id: String,
    content: String,
    out_tx: mpsc::Sender<ServerWsMessage>,
    cancel_token: CancellationToken,
) {
    let request = proto::HandleMessageRequest {
        user_id,
        conversation_id: conversation_id.clone(),
        content,
    };

    let response = match agent_client.handle_message(request).await {
        Ok(response) => response,
        Err(e) => {
            let _ = out_tx
                .send(ServerWsMessage::ChatError {
                    conversation_id,
                    error: e.message().to_owned(),
                })
                .await;
            return;
        }
    };

    let mut stream = response.into_inner();

    loop {
        tokio::select! {
            () = cancel_token.cancelled() => {
                break;
            }
            event = stream.next() => {
                let event: proto::AgentEvent = match event {
                    Some(Ok(e)) => e,
                    Some(Err(e)) => {
                        let _ = out_tx
                            .send(ServerWsMessage::ChatError {
                                conversation_id: conversation_id.clone(),
                                error: e.message().to_owned(),
                            })
                            .await;
                        break;
                    }
                    None => break,
                };

                let ws_msg = match event.event {
                    Some(proto::agent_event::Event::TextDelta(td)) => {
                        ServerWsMessage::ChatDelta {
                            conversation_id: conversation_id.clone(),
                            content: td.content,
                        }
                    }
                    Some(proto::agent_event::Event::ToolCallStart(tcs)) => {
                        ServerWsMessage::ChatToolUse {
                            conversation_id: conversation_id.clone(),
                            tool_call: serde_json::json!({
                                "name": tcs.name,
                                "input": tcs.input_json,
                            }),
                        }
                    }
                    Some(proto::agent_event::Event::ToolCallResult(tcr)) => {
                        ServerWsMessage::ChatToolResult {
                            conversation_id: conversation_id.clone(),
                            tool_call_id: tcr.name.clone(),
                            output: tcr.output,
                        }
                    }
                    Some(proto::agent_event::Event::Done(done)) => {
                        ServerWsMessage::ChatDone {
                            conversation_id: conversation_id.clone(),
                            message_id: done.message_id,
                        }
                    }
                    Some(proto::agent_event::Event::ThinkingDelta(td)) => {
                        ServerWsMessage::ChatThinking {
                            conversation_id: conversation_id.clone(),
                            content: td.content,
                        }
                    }
                    Some(proto::agent_event::Event::TitleGenerated(tg)) => {
                        ServerWsMessage::ChatTitle {
                            conversation_id: conversation_id.clone(),
                            title: tg.title,
                        }
                    }
                    Some(proto::agent_event::Event::Error(e)) => {
                        ServerWsMessage::ChatError {
                            conversation_id: conversation_id.clone(),
                            error: e.message,
                        }
                    }
                    None => continue,
                };

                if out_tx.send(ws_msg).await.is_err() {
                    break;
                }
            }
        }
    }
}
