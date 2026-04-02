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
use sober_core::types::{ContentBlock, ConversationId, ConversationUserRepo, MessageRepo};
use sober_db::{PgConversationUserRepo, PgMessageRepo};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::proto;
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

    // Channel for sending messages back to the client from the connection registry.
    let (out_tx, mut out_rx) = mpsc::channel::<ServerWsMessage>(64);

    // Register the user's connection for cross-conversation events (unread notifications).
    state
        .user_connections
        .register(&user_id.to_string(), out_tx.clone())
        .await;

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
                    metrics::counter!("sober_api_ws_messages_total", "direction" => "outbound")
                        .increment(1);
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
                metrics::counter!("sober_api_ws_connections_total", "status" => "error")
                    .increment(1);
                break;
            }
        };

        metrics::counter!("sober_api_ws_messages_total", "direction" => "inbound").increment(1);

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
                // Verify membership before subscribing.
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

                if super::verify_membership(&state.db, conv_id, auth_user.user_id)
                    .await
                    .is_err()
                {
                    let _ = out_tx
                        .send(ServerWsMessage::ChatError {
                            conversation_id,
                            error: "not a member of this conversation".into(),
                        })
                        .await;
                    continue;
                }

                // Register this connection for the conversation so events
                // from the subscription task are routed here (e.g. scheduler results).
                if registered_conversations.insert(conversation_id.clone()) {
                    state
                        .connections
                        .register(conversation_id.clone(), out_tx.clone())
                        .await;
                }

                // Mark conversation as read for this user (best-effort).
                {
                    let cu_repo = PgConversationUserRepo::new(state.db.clone());
                    let msg_repo = PgMessageRepo::new(state.db.clone());
                    if let Ok(messages) = msg_repo.list_paginated(conv_id, None, 1).await
                        && let Some(latest) = messages.first()
                    {
                        cu_repo
                            .mark_read(conv_id, auth_user.user_id, latest.id)
                            .await
                            .ok();
                    }
                }
            }
            ClientWsMessage::ChatMessage {
                conversation_id,
                content,
            } => {
                // Verify membership before sending message.
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

                if super::verify_membership(&state.db, conv_id, auth_user.user_id)
                    .await
                    .is_err()
                {
                    let _ = out_tx
                        .send(ServerWsMessage::ChatError {
                            conversation_id,
                            error: "not a member of this conversation".into(),
                        })
                        .await;
                    continue;
                }

                // Ensure registration (in case chat.subscribe wasn't sent first).
                if registered_conversations.insert(conversation_id.clone()) {
                    state
                        .connections
                        .register(conversation_id.clone(), out_tx.clone())
                        .await;
                }

                // Broadcast the user's message to all other subscribers
                // so group members see it in real-time.
                let user_msg = ServerWsMessage::ChatNewMessage {
                    conversation_id: conversation_id.clone(),
                    message_id: uuid::Uuid::now_v7().to_string(),
                    role: "user".into(),
                    content: content.clone(),
                    source: sober_core::types::access::TriggerKind::Human,
                    user_id: Some(user_id.to_string()),
                    username: Some(username.clone()),
                };
                state.connections.send(&conversation_id, user_msg).await;

                // Notify all subscribers that the agent is processing.
                state
                    .connections
                    .send(
                        &conversation_id,
                        ServerWsMessage::ChatAgentTyping {
                            conversation_id: conversation_id.clone(),
                        },
                    )
                    .await;

                // Convert content blocks to proto format.
                let proto_blocks: Vec<proto::ContentBlock> = content
                    .into_iter()
                    .map(|block| match block {
                        ContentBlock::Text { text } => proto::ContentBlock {
                            block: Some(proto::content_block::Block::Text(proto::TextBlock {
                                text,
                            })),
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
                    })
                    .collect();

                // Call unary HandleMessage RPC — fire and forget.
                let mut agent_client = state.agent_client.clone();
                let mut request = tonic::Request::new(proto::HandleMessageRequest {
                    user_id: user_id.to_string(),
                    conversation_id: conversation_id.clone(),
                    content: proto_blocks,
                });

                let conv_id = conversation_id.clone();
                let error_tx = out_tx.clone();
                let span = tracing::info_span!(
                    "ws.handle_message",
                    otel.kind = "client",
                    rpc.service = "AgentService",
                    rpc.method = "HandleMessage",
                    rpc.system = "grpc",
                    user.id = %user_id,
                    conversation.id = %conv_id,
                    otel.status_code = tracing::field::Empty,
                );
                // Inject the new span's trace context into the gRPC metadata
                // so the agent can link its work to this trace.
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
                                tracing::error!(
                                    error.message = %e.message(),
                                    error.type = %e.code(),
                                    "HandleMessage RPC failed"
                                );
                                let _ = error_tx
                                    .send(ServerWsMessage::ChatError {
                                        conversation_id: conv_id,
                                        error: e.message().to_owned(),
                                    })
                                    .await;
                            }
                        }
                    },
                    span,
                ));
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

    // Unregister user-level connection.
    state
        .user_connections
        .unregister(&user_id.to_string())
        .await;

    send_task.abort();

    metrics::gauge!("sober_api_ws_connections_active").decrement(1);
    metrics::counter!("sober_api_ws_connections_total", "status" => "closed").increment(1);

    info!(user_id = %user_id, "WebSocket disconnected");
}
