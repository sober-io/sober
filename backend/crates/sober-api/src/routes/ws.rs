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
use sober_core::types::{ContentBlock, ConversationId};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::state::AppState;
use crate::ws_types::ServerWsMessage;

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
        content: Vec<ContentBlock>,
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

/// Handles a single WebSocket connection.
async fn handle_socket(socket: WebSocket, state: Arc<AppState>, auth_user: AuthUser) {
    let user_id = auth_user.user_id;
    info!(user_id = %user_id, "WebSocket connected");

    metrics::gauge!("sober_api_ws_connections_active").increment(1);
    metrics::counter!("sober_api_ws_connections_total", "status" => "opened").increment(1);

    // Look up username once for group message attribution.
    let username = {
        use sober_core::types::UserRepo;
        let user_repo = sober_db::PgUserRepo::new(state.db.clone());
        user_repo
            .get_by_id(user_id)
            .await
            .map(|u| u.username)
            .unwrap_or_default()
    };

    let (mut ws_tx, mut ws_rx) = socket.split();

    let (out_tx, mut out_rx) = mpsc::channel::<ServerWsMessage>(64);

    // Register the user's connection for cross-conversation events.
    state
        .user_connections
        .register(&user_id.to_string(), out_tx.clone())
        .await;

    let mut registered_conversations: HashSet<String> = HashSet::new();

    // Spawn a task that forwards outbound messages to the WebSocket.
    let send_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            match serde_json::to_string(&msg) {
                Ok(text) => {
                    if ws_tx.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                    metrics::counter!("sober_api_ws_messages_total", "direction" => "outbound")
                        .increment(1);
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to serialize WebSocket message");
                }
            }
        }
    });

    // Process incoming messages.
    while let Some(msg) = ws_rx.next().await {
        let msg = match msg {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(e) => {
                warn!(error = %e, "WebSocket receive error");
                metrics::counter!("sober_api_ws_connections_total", "status" => "error")
                    .increment(1);
                break;
            }
        };

        metrics::counter!("sober_api_ws_messages_total", "direction" => "inbound").increment(1);

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
                let conv_id = match conversation_id
                    .parse::<uuid::Uuid>()
                    .map(ConversationId::from_uuid)
                {
                    Ok(id) => id,
                    Err(_) => {
                        let _ = out_tx
                            .send(ServerWsMessage::ChatError {
                                conversation_id,
                                error: "invalid conversation_id".into(),
                            })
                            .await;
                        continue;
                    }
                };

                if state.ws_dispatch.subscribe(conv_id, user_id).await.is_err() {
                    let _ = out_tx
                        .send(ServerWsMessage::ChatError {
                            conversation_id,
                            error: "not a member of this conversation".into(),
                        })
                        .await;
                    continue;
                }

                if registered_conversations.insert(conversation_id.clone()) {
                    state
                        .connections
                        .register(conversation_id, out_tx.clone())
                        .await;
                }
            }
            ClientWsMessage::ChatMessage {
                conversation_id,
                content,
            } => {
                let conv_id = match conversation_id
                    .parse::<uuid::Uuid>()
                    .map(ConversationId::from_uuid)
                {
                    Ok(id) => id,
                    Err(_) => {
                        let _ = out_tx
                            .send(ServerWsMessage::ChatError {
                                conversation_id,
                                error: "invalid conversation_id".into(),
                            })
                            .await;
                        continue;
                    }
                };

                // Ensure registration.
                if registered_conversations.insert(conversation_id.clone()) {
                    state
                        .connections
                        .register(conversation_id.clone(), out_tx.clone())
                        .await;
                }

                if let Err(e) = state
                    .ws_dispatch
                    .send_message(conv_id, user_id, &username, content)
                    .await
                {
                    let _ = out_tx
                        .send(ServerWsMessage::ChatError {
                            conversation_id,
                            error: e.to_string(),
                        })
                        .await;
                }
            }
            ClientWsMessage::ChatCancel { conversation_id } => {
                info!(conversation_id, "chat cancel requested (best-effort)");
            }
            ClientWsMessage::ChatConfirmResponse {
                conversation_id: _,
                confirm_id,
                approved,
            } => {
                let _ = state
                    .ws_dispatch
                    .confirm_response(confirm_id, approved)
                    .await;
            }
            ClientWsMessage::ChatSetPermissionMode {
                conversation_id: _,
                mode,
            } => {
                let _ = state.ws_dispatch.set_permission_mode(mode).await;
            }
        }
    }

    // Unregister all conversations on disconnect.
    for conv_id in &registered_conversations {
        state.connections.unregister(conv_id).await;
    }

    state
        .user_connections
        .unregister(&user_id.to_string())
        .await;

    send_task.abort();

    metrics::gauge!("sober_api_ws_connections_active").decrement(1);
    metrics::counter!("sober_api_ws_connections_total", "status" => "closed").increment(1);

    info!(user_id = %user_id, "WebSocket disconnected");
}
