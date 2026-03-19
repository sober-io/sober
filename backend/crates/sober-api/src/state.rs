//! Application state shared across all request handlers.
//!
//! [`AppState`] holds the database pool, gRPC agent client, and
//! configuration. Wrapped in `Arc` and injected via axum `State`.

use std::sync::Arc;

use hyper_util::rt::TokioIo;
use sober_auth::AuthService;
use sober_core::config::AppConfig;
use sober_core::error::AppError;
use sober_db::{PgRoleRepo, PgSessionRepo, PgUserRepo};
use sqlx::PgPool;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use tracing::info;

use crate::connections::{ConnectionRegistry, UserConnectionRegistry};
use crate::proto;

/// gRPC client for the agent service, connected via Unix domain socket.
pub type AgentClient = proto::agent_service_client::AgentServiceClient<Channel>;

/// Application state shared across handlers via `axum::extract::State`.
pub struct AppState {
    /// PostgreSQL connection pool.
    pub db: PgPool,
    /// gRPC client for the agent service.
    pub agent_client: AgentClient,
    /// Authentication service.
    pub auth: Arc<AuthService<PgUserRepo, PgSessionRepo, PgRoleRepo>>,
    /// Application configuration.
    pub config: AppConfig,
    /// Registry of active WebSocket connections per conversation.
    pub connections: ConnectionRegistry,
    /// Registry of active WebSocket connections per user (for unread notifications).
    pub user_connections: UserConnectionRegistry,
}

impl AppState {
    /// Constructs application state from pre-existing components.
    ///
    /// Used in integration tests where the pool comes from `#[sqlx::test]`,
    /// the agent client connects to a mock gRPC server, and the auth service
    /// is shared with test helpers.
    pub fn from_parts(
        db: PgPool,
        agent_client: AgentClient,
        auth: Arc<AuthService<PgUserRepo, PgSessionRepo, PgRoleRepo>>,
        config: AppConfig,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            agent_client,
            auth,
            config,
            connections: ConnectionRegistry::new(),
            user_connections: UserConnectionRegistry::new(),
        })
    }

    /// Constructs application state by connecting to PostgreSQL and the
    /// agent gRPC service. Fails fast on connection errors.
    pub async fn new(config: AppConfig) -> Result<Arc<Self>, AppError> {
        let db_config = sober_db::DatabaseConfig {
            url: config.database.url.clone(),
            max_connections: config.database.max_connections,
        };
        let db = sober_db::create_pool_with_service(&db_config, "sober-api").await?;
        info!("connected to PostgreSQL");

        let agent_client = connect_agent(&config).await?;
        info!("connected to agent gRPC service");

        let users = PgUserRepo::new(db.clone());
        let sessions = PgSessionRepo::new(db.clone());
        let roles = PgRoleRepo::new(db.clone());
        let auth = Arc::new(AuthService::new(
            users,
            sessions,
            roles,
            config.auth.session_ttl_seconds,
        ));

        Ok(Arc::new(Self {
            db,
            agent_client,
            auth,
            config,
            connections: ConnectionRegistry::new(),
            user_connections: UserConnectionRegistry::new(),
        }))
    }
}

/// Connects to the agent gRPC service over a Unix domain socket.
///
/// The returned client includes an interceptor that injects W3C TraceContext
/// headers into every outgoing request for cross-service trace propagation.
async fn connect_agent(config: &AppConfig) -> Result<AgentClient, AppError> {
    let socket_path = config.scheduler.agent_socket_path.clone();

    // tonic requires a valid URI even for UDS connections. The host and
    // port below are never used — `connect_with_connector` bypasses them
    // entirely and connects to `socket_path` instead.
    let channel = Endpoint::try_from("http://[::]:50051")
        .map_err(|e| AppError::Internal(e.into()))?
        .connect_with_connector(service_fn(move |_: Uri| {
            let path = socket_path.clone();
            async move {
                let stream = tokio::net::UnixStream::connect(path).await?;
                Ok::<_, std::io::Error>(TokioIo::new(stream))
            }
        }))
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    Ok(AgentClient::new(channel))
}
