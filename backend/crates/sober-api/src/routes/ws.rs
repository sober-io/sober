//! WebSocket chat handler.
//!
//! Single endpoint at `/api/v1/ws`. All messages include `conversation_id`
//! in the payload for multiplexing across conversations on one connection.
//!
//! Events are delivered via the background subscription task that routes
//! `ConversationUpdate` events from the agent through the
//! [`ConnectionRegistry`](crate::connections::ConnectionRegistry).

use std::collections::HashSet;
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use futures::{SinkExt, StreamExt};
use sober_auth::AuthUser;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::proto;
use crate::state::AppState;

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
#[expect(clippy::enum_variant_names)]
enum ClientWsMessage {
    #[serde(rename = "chat.subscribe")]
    ChatSubscribe { conversation_id: String },
    #[serde(rename = "chat.message")]
    ChatMessage {
        conversation_id: String,
        content: String,
    },
    #[serde(rename = "chat.cancel")]
    ChatCancel { conversation_id: String },
    #[serde(rename = "chat.confirm_response")]
    ChatConfirmResponse {
        #[expect(dead_code)]
        conversation_id: String,
        confirm_id: String,
        approved: bool,
    },
    #[serde(rename = "chat.set_permission_mode")]
    ChatSetPermissionMode {
        #[expect(dead_code)]
        conversation_id: String,
        mode: String,
    },
}

/// Server-to-client WebSocket message types.
#[derive(serde::Serialize, Clone)]
#[serde(tag = "type")]
pub enum ServerWsMessage {
    /// Incremental text from the assistant.
    #[serde(rename = "chat.delta")]
    ChatDelta {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// Text fragment.
        content: String,
    },
    /// A tool call has started.
    #[serde(rename = "chat.tool_use")]
    ChatToolUse {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// Tool call details.
        tool_call: serde_json::Value,
    },
    /// A tool call has completed.
    #[serde(rename = "chat.tool_result")]
    ChatToolResult {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// Name of the tool.
        tool_call_id: String,
        /// Output from the tool.
        output: String,
    },
    /// The agent has finished processing.
    #[serde(rename = "chat.done")]
    ChatDone {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// ID of the stored assistant message.
        message_id: String,
    },
    /// Thinking/reasoning content from the model.
    #[serde(rename = "chat.thinking")]
    ChatThinking {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// Thinking text fragment.
        content: String,
    },
    /// The conversation title was generated or changed.
    #[serde(rename = "chat.title")]
    ChatTitle {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// The new title.
        title: String,
    },
    /// An error occurred.
    #[serde(rename = "chat.error")]
    ChatError {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// Error description.
        error: String,
    },
    /// A shell command confirmation request.
    #[serde(rename = "chat.confirm")]
    ChatConfirm {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// Unique ID for this confirmation request.
        confirm_id: String,
        /// The command that needs approval.
        command: String,
        /// Risk level assessment.
        risk_level: String,
        /// Resources affected.
        affects: Vec<String>,
        /// Reason for requiring confirmation.
        reason: String,
    },
    /// A new message was stored in the conversation.
    #[serde(rename = "chat.new_message")]
    ChatNewMessage {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// ID of the stored message.
        message_id: String,
        /// Role of the message author.
        role: String,
        /// Message content.
        content: String,
        /// What produced this message.
        source: sober_core::types::access::TriggerKind,
    },
    /// Keepalive response.
    #[serde(rename = "pong")]
    Pong,
}

/// Handles a single WebSocket connection.
async fn handle_socket(socket: WebSocket, state: Arc<AppState>, auth_user: AuthUser) {
    let user_id = auth_user.user_id;
    info!(user_id = %user_id, "WebSocket connected");

    let (mut ws_tx, mut ws_rx) = socket.split();

    // Channel for sending messages back to the client from the connection registry.
    let (out_tx, mut out_rx) = mpsc::channel::<ServerWsMessage>(64);

    // Track which conversations this connection is registered for.
    let mut registered_conversations: HashSet<String> = HashSet::new();

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

        // Handle keepalive pings from the client.
        if msg.as_str() == "ping" {
            let _ = out_tx.send(ServerWsMessage::Pong).await;
            continue;
        }

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
            ClientWsMessage::ChatSubscribe { conversation_id } => {
                // Register this connection for the conversation so events
                // from the subscription task are routed here (e.g. scheduler results).
                if registered_conversations.insert(conversation_id.clone()) {
                    state
                        .connections
                        .register(conversation_id.clone(), out_tx.clone())
                        .await;
                }
            }
            ClientWsMessage::ChatMessage {
                conversation_id,
                content,
            } => {
                // Ensure registration (in case chat.subscribe wasn't sent first).
                if registered_conversations.insert(conversation_id.clone()) {
                    state
                        .connections
                        .register(conversation_id.clone(), out_tx.clone())
                        .await;
                }

                // Call unary HandleMessage RPC — fire and forget.
                let mut agent_client = state.agent_client.clone();
                let request = proto::HandleMessageRequest {
                    user_id: user_id.to_string(),
                    conversation_id: conversation_id.clone(),
                    content,
                };

                let conv_id = conversation_id.clone();
                let error_tx = out_tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = agent_client.handle_message(request).await {
                        let _ = error_tx
                            .send(ServerWsMessage::ChatError {
                                conversation_id: conv_id,
                                error: e.message().to_owned(),
                            })
                            .await;
                    }
                });
            }
            ClientWsMessage::ChatCancel { conversation_id } => {
                // Cancellation is best-effort in the new model.
                // The agent loop will continue, but we can unregister
                // the connection from the conversation.
                info!(conversation_id, "chat cancel requested (best-effort)");
            }
            ClientWsMessage::ChatConfirmResponse {
                conversation_id: _,
                confirm_id,
                approved,
            } => {
                let mut agent_client = state.agent_client.clone();
                let resp = proto::ConfirmResponse {
                    confirm_id,
                    approved,
                };
                if let Err(e) = agent_client.submit_confirmation(resp).await {
                    warn!(error = %e, "failed to submit confirmation");
                }
            }
            ClientWsMessage::ChatSetPermissionMode {
                conversation_id: _,
                mode,
            } => {
                let mut agent_client = state.agent_client.clone();
                let req = proto::SetPermissionModeRequest { mode: mode.clone() };
                if let Err(e) = agent_client.set_permission_mode(req).await {
                    warn!(error = %e, mode, "failed to set permission mode");
                }
            }
        }
    }

    // Unregister all conversations on disconnect.
    for conv_id in &registered_conversations {
        state.connections.unregister(conv_id).await;
    }

    send_task.abort();
    info!(user_id = %user_id, "WebSocket disconnected");
}
