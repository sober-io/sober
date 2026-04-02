//! Application state shared across all request handlers.
//!
//! [`AppState`] holds the database pool, gRPC agent client, and
//! configuration. Wrapped in `Arc` and injected via axum `State`.

use std::sync::Arc;

use hyper_util::rt::TokioIo;
use sober_auth::AuthService as SoberAuthService;
use sober_core::config::AppConfig;
use sober_core::error::AppError;
use sober_db::{PgRoleRepo, PgSessionRepo, PgUserRepo};
use sober_workspace::BlobStore;
use sqlx::PgPool;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use tracing::info;

use crate::connections::{ConnectionRegistry, UserConnectionRegistry};
use crate::gateway_proto;
use crate::proto;
use crate::services::{
    attachment::AttachmentService, auth::AuthService, collaborator::CollaboratorService,
    conversation::ConversationService, evolution::EvolutionService, gateway::GatewayAdminService,
    message::MessageService, plugin::PluginService, tag::TagService, user::UserService,
    ws_dispatch::WsDispatchService,
};

/// gRPC client for the agent service, connected via Unix domain socket.
pub type AgentClient = proto::agent_service_client::AgentServiceClient<Channel>;

/// gRPC client for the gateway service, connected via Unix domain socket.
pub type GatewayClient = gateway_proto::gateway_service_client::GatewayServiceClient<Channel>;

/// Application state shared across handlers via `axum::extract::State`.
pub struct AppState {
    /// PostgreSQL connection pool.
    pub db: PgPool,
    /// gRPC client for the agent service.
    pub agent_client: AgentClient,
    /// Authentication service.
    pub auth: Arc<SoberAuthService<PgUserRepo, PgSessionRepo, PgRoleRepo>>,
    /// Application configuration.
    pub config: AppConfig,
    /// Content-addressed blob store for attachment files.
    pub blob_store: Arc<BlobStore>,
    /// Registry of active WebSocket connections per conversation.
    pub connections: ConnectionRegistry,
    /// Registry of active WebSocket connections per user (for unread notifications).
    pub user_connections: UserConnectionRegistry,
    /// Tag management service.
    pub tag: Arc<TagService>,
    /// User search service.
    pub user: Arc<UserService>,
    /// Conversation lifecycle service.
    pub conversation: Arc<ConversationService>,
    /// Collaborator management service.
    pub collaborator: Arc<CollaboratorService>,
    /// Message listing and deletion service.
    pub message: Arc<MessageService>,
    /// WebSocket dispatch service.
    pub ws_dispatch: Arc<WsDispatchService>,
    /// Plugin management service.
    pub plugin: Arc<PluginService>,
    /// Evolution lifecycle service.
    pub evolution: Arc<EvolutionService>,
    /// Attachment upload service.
    pub attachment: Arc<AttachmentService>,
    /// API-level auth service (inbox creation, user profile).
    pub auth_service: Arc<AuthService>,
    /// Gateway admin service (platform/mapping CRUD).
    pub gateway_admin: Arc<GatewayAdminService>,
    /// Optional gRPC client for the gateway service.
    pub gateway_client: Option<GatewayClient>,
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
        auth: Arc<SoberAuthService<PgUserRepo, PgSessionRepo, PgRoleRepo>>,
        config: AppConfig,
    ) -> Arc<Self> {
        let blob_root = config
            .workspace_root
            .join(sober_workspace::SOBER_DIR)
            .join("blobs");
        let blob_store = Arc::new(BlobStore::new(blob_root));
        let connections = ConnectionRegistry::new();
        let user_connections = UserConnectionRegistry::new();

        let tag = Arc::new(TagService::new(db.clone()));
        let user = Arc::new(UserService::new(db.clone()));
        let conversation = Arc::new(ConversationService::new(db.clone(), config.clone()));
        let collaborator = Arc::new(CollaboratorService::new(
            db.clone(),
            user_connections.clone(),
        ));
        let message = Arc::new(MessageService::new(db.clone()));
        let ws_dispatch = Arc::new(WsDispatchService::new(
            db.clone(),
            agent_client.clone(),
            connections.clone(),
        ));
        let plugin = Arc::new(PluginService::new(db.clone(), agent_client.clone()));
        let evolution = Arc::new(EvolutionService::new(
            db.clone(),
            agent_client.clone(),
            config.clone(),
        ));
        let attachment = Arc::new(AttachmentService::new(db.clone(), blob_store.clone()));
        let auth_service = Arc::new(AuthService::new(db.clone()));
        let gateway_admin = Arc::new(GatewayAdminService::new(db.clone()));

        Arc::new(Self {
            db,
            agent_client,
            auth,
            config,
            blob_store,
            connections,
            user_connections,
            tag,
            user,
            conversation,
            collaborator,
            message,
            ws_dispatch,
            plugin,
            evolution,
            attachment,
            auth_service,
            gateway_admin,
            gateway_client: None,
        })
    }

    /// Constructs application state by connecting to PostgreSQL and the
    /// agent gRPC service. Fails fast on connection errors.
    pub async fn new(config: AppConfig) -> Result<Arc<Self>, AppError> {
        let db_config = sober_db::DatabaseConfig {
            url: config.database.url.clone(),
            max_connections: config.database.max_connections,
        };
        let db = sober_db::create_pool(&db_config).await?;
        info!("connected to PostgreSQL");

        let agent_client = connect_agent(&config).await?;
        info!("connected to agent gRPC service");

        let users = PgUserRepo::new(db.clone());
        let sessions = PgSessionRepo::new(db.clone());
        let roles = PgRoleRepo::new(db.clone());
        let auth = Arc::new(SoberAuthService::new(
            users,
            sessions,
            roles,
            config.auth.session_ttl_seconds,
        ));

        let blob_root = config
            .workspace_root
            .join(sober_workspace::SOBER_DIR)
            .join("blobs");
        let blob_store = Arc::new(BlobStore::new(blob_root));
        let connections = ConnectionRegistry::new();
        let user_connections = UserConnectionRegistry::new();

        let tag = Arc::new(TagService::new(db.clone()));
        let user = Arc::new(UserService::new(db.clone()));
        let conversation = Arc::new(ConversationService::new(db.clone(), config.clone()));
        let collaborator = Arc::new(CollaboratorService::new(
            db.clone(),
            user_connections.clone(),
        ));
        let message = Arc::new(MessageService::new(db.clone()));
        let ws_dispatch = Arc::new(WsDispatchService::new(
            db.clone(),
            agent_client.clone(),
            connections.clone(),
        ));
        let plugin = Arc::new(PluginService::new(db.clone(), agent_client.clone()));
        let evolution = Arc::new(EvolutionService::new(
            db.clone(),
            agent_client.clone(),
            config.clone(),
        ));
        let attachment = Arc::new(AttachmentService::new(db.clone(), blob_store.clone()));
        let auth_service = Arc::new(AuthService::new(db.clone()));
        let gateway_admin = Arc::new(GatewayAdminService::new(db.clone()));

        let gateway_client = match connect_gateway(&config).await {
            Ok(client) => {
                info!("connected to gateway gRPC service");
                Some(client)
            }
            Err(e) => {
                info!(error = %e, "gateway not available (optional)");
                None
            }
        };

        Ok(Arc::new(Self {
            db,
            agent_client,
            auth,
            config,
            blob_store,
            connections,
            user_connections,
            tag,
            user,
            conversation,
            collaborator,
            message,
            ws_dispatch,
            plugin,
            evolution,
            attachment,
            auth_service,
            gateway_admin,
            gateway_client,
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

/// Connects to the gateway gRPC service over a Unix domain socket.
///
/// Returns an error if the socket is not available; callers treat this as
/// optional and store `None` rather than failing startup.
async fn connect_gateway(config: &AppConfig) -> Result<GatewayClient, AppError> {
    let socket_path = config.gateway.socket_path.clone();

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

    Ok(GatewayClient::new(channel))
}
