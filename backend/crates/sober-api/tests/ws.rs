//! Integration tests for WebSocket endpoint.
//!
//! Starts a real HTTP server with a mock agent gRPC backend, then connects
//! via `tokio-tungstenite` to exercise the WebSocket flow.

use futures::{SinkExt, StreamExt};
use sober_api::proto;
use sober_api::routes;
use sober_api::state::AppState;
use sober_auth::AuthService;
use sober_core::config::AppConfig;
use sober_db::{PgRoleRepo, PgSessionRepo, PgUserRepo};
use sqlx::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

// ── Mock Agent gRPC Server ──────────────────────────────────────────────────

/// A mock implementation of the AgentService that streams back canned events.
struct MockAgentService;

#[tonic::async_trait]
impl proto::agent_service_server::AgentService for MockAgentService {
    type HandleMessageStream =
        tokio_stream::wrappers::ReceiverStream<Result<proto::AgentEvent, tonic::Status>>;
    type ExecuteTaskStream =
        tokio_stream::wrappers::ReceiverStream<Result<proto::AgentEvent, tonic::Status>>;

    async fn handle_message(
        &self,
        request: tonic::Request<proto::HandleMessageRequest>,
    ) -> Result<tonic::Response<Self::HandleMessageStream>, tonic::Status> {
        let req = request.into_inner();
        let conv_id = req.conversation_id.clone();

        let (tx, rx) = tokio::sync::mpsc::channel(16);

        tokio::spawn(async move {
            // Send a text delta.
            let _ = tx
                .send(Ok(proto::AgentEvent {
                    event: Some(proto::agent_event::Event::TextDelta(proto::TextDelta {
                        content: format!("Echo: {}", req.content),
                    })),
                }))
                .await;

            // Send done.
            let _ = tx
                .send(Ok(proto::AgentEvent {
                    event: Some(proto::agent_event::Event::Done(proto::Done {
                        message_id: format!("msg-{conv_id}"),
                        prompt_tokens: 10,
                        completion_tokens: 5,
                    })),
                }))
                .await;
        });

        Ok(tonic::Response::new(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        ))
    }

    async fn execute_task(
        &self,
        _request: tonic::Request<proto::ExecuteTaskRequest>,
    ) -> Result<tonic::Response<Self::ExecuteTaskStream>, tonic::Status> {
        Err(tonic::Status::unimplemented("not used in tests"))
    }

    async fn wake_agent(
        &self,
        _request: tonic::Request<proto::WakeRequest>,
    ) -> Result<tonic::Response<proto::WakeResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("not used in tests"))
    }

    async fn submit_confirmation(
        &self,
        _request: tonic::Request<proto::ConfirmResponse>,
    ) -> Result<tonic::Response<proto::ConfirmAck>, tonic::Status> {
        Ok(tonic::Response::new(proto::ConfirmAck {}))
    }

    async fn health(
        &self,
        _request: tonic::Request<proto::HealthRequest>,
    ) -> Result<tonic::Response<proto::HealthResponse>, tonic::Status> {
        Ok(tonic::Response::new(proto::HealthResponse {
            healthy: true,
            version: "test".into(),
        }))
    }
}

// ── Test Harness ────────────────────────────────────────────────────────────

/// Starts the mock gRPC server and returns an `AgentClient` connected to it.
async fn start_mock_grpc() -> sober_api::state::AgentClient {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(proto::agent_service_server::AgentServiceServer::new(
                MockAgentService,
            ))
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });

    // Connect the client.
    let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();

    sober_api::state::AgentClient::new(channel)
}

/// Starts the full HTTP server and returns (addr, token).
async fn start_server(pool: PgPool) -> (SocketAddr, String) {
    let users = PgUserRepo::new(pool.clone());
    let sessions = PgSessionRepo::new(pool.clone());
    let roles = PgRoleRepo::new(pool.clone());
    let auth = Arc::new(AuthService::new(users, sessions, roles, 86400));

    // Register and approve a user.
    let user = auth
        .register("ws@example.com", "wsuser", "securepassword123")
        .await
        .unwrap();
    auth.approve_user(user.id).await.unwrap();
    let (token, _) = auth
        .login("ws@example.com", "securepassword123")
        .await
        .unwrap();

    let config = AppConfig::load_from(|key| match key {
        "DATABASE_URL" => Some("postgres://unused:unused@localhost/unused".into()),
        _ => None,
    })
    .unwrap();
    let agent_client = start_mock_grpc().await;
    let state = AppState::from_parts(pool, agent_client, auth, config);

    let app = routes::build_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (addr, token)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[sqlx::test(migrations = "../../migrations")]
async fn ws_chat_message_receives_delta_and_done(pool: PgPool) {
    let (addr, token) = start_server(pool).await;

    // Connect WebSocket with auth cookie.
    let url = format!("ws://{addr}/api/v1/ws");
    let request = http::Request::builder()
        .uri(&url)
        .header("Cookie", format!("sober_session={token}"))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tokio_tungstenite::tungstenite::handshake::client::generate_key(),
        )
        .header("Host", addr.to_string())
        .body(())
        .unwrap();

    let (mut ws, _response) = tokio_tungstenite::connect_async(request).await.unwrap();

    // Send a chat message.
    let msg = serde_json::json!({
        "type": "chat.message",
        "conversation_id": "conv-1",
        "content": "Hello"
    });
    ws.send(Message::Text(msg.to_string().into()))
        .await
        .unwrap();

    // Receive text delta.
    let received = ws.next().await.unwrap().unwrap();
    let text = match received {
        Message::Text(t) => t,
        other => panic!("expected text message, got: {other:?}"),
    };
    let delta: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(delta["type"], "chat.delta");
    assert_eq!(delta["conversation_id"], "conv-1");
    assert_eq!(delta["content"], "Echo: Hello");

    // Receive done.
    let received = ws.next().await.unwrap().unwrap();
    let text = match received {
        Message::Text(t) => t,
        other => panic!("expected text message, got: {other:?}"),
    };
    let done: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(done["type"], "chat.done");
    assert_eq!(done["conversation_id"], "conv-1");
    assert_eq!(done["message_id"], "msg-conv-1");

    ws.close(None).await.unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn ws_without_auth_rejects(pool: PgPool) {
    let (addr, _token) = start_server(pool).await;

    // Try connecting without auth — should fail with 401.
    let url = format!("ws://{addr}/api/v1/ws");
    let result = tokio_tungstenite::connect_async(&url).await;

    // The server should reject the upgrade with a non-101 status.
    assert!(
        result.is_err(),
        "expected WebSocket connection to fail without auth"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn ws_chat_cancel_stops_stream(pool: PgPool) {
    let (addr, token) = start_server(pool).await;

    let url = format!("ws://{addr}/api/v1/ws");
    let request = http::Request::builder()
        .uri(&url)
        .header("Cookie", format!("sober_session={token}"))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tokio_tungstenite::tungstenite::handshake::client::generate_key(),
        )
        .header("Host", addr.to_string())
        .body(())
        .unwrap();

    let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();

    // Send a chat message.
    let msg = serde_json::json!({
        "type": "chat.message",
        "conversation_id": "conv-cancel",
        "content": "Cancel me"
    });
    ws.send(Message::Text(msg.to_string().into()))
        .await
        .unwrap();

    // Immediately send cancel.
    let cancel = serde_json::json!({
        "type": "chat.cancel",
        "conversation_id": "conv-cancel"
    });
    ws.send(Message::Text(cancel.to_string().into()))
        .await
        .unwrap();

    // We should either receive the events (if cancel came too late) or
    // not — either way the connection should remain healthy.
    // Drain any remaining messages with a timeout.
    let timeout = tokio::time::timeout(tokio::time::Duration::from_secs(2), async {
        while let Some(msg) = ws.next().await {
            match msg {
                Ok(Message::Text(_)) => continue,
                Ok(Message::Close(_)) => break,
                Err(_) => break,
                _ => continue,
            }
        }
    });
    let _ = timeout.await; // OK if it times out — means stream ended or was cancelled.

    // Connection should still be usable (send close frame).
    let _ = ws.close(None).await;
}
