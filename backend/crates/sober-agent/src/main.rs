//! Sober Agent — gRPC server for agent orchestration, replica lifecycle, and
//! task delegation.
//!
//! This binary starts the agent gRPC service on a Unix domain socket. It is
//! called by `sober-api` and `sober-scheduler` at runtime.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use sober_agent::ConfirmationBroker;
use sober_agent::agent::{Agent, AgentConfig};
use sober_agent::grpc::AgentGrpcService;
use sober_agent::grpc::proto::agent_service_server::AgentServiceServer;
use sober_agent::tools::{FetchUrlTool, ShellTool, ToolRegistry, WebSearchTool};
use sober_core::PermissionMode;
use sober_core::config::AppConfig;
use sober_core::types::tool::Tool;
use sober_db::{PgConversationRepo, PgMcpServerRepo, PgMessageRepo, create_pool};
use sober_llm::OpenAiCompatibleEngine;
use sober_memory::{ContextLoader, MemoryStore};
use sober_mind::assembly::Mind;
use sober_mind::soul::SoulResolver;
use sober_sandbox::{CommandPolicy, SandboxProfile};
use std::sync::RwLock;
use tokio::net::UnixListener;
use tokio::signal;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::Server;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Load .env and configuration
    let config = AppConfig::load_from_env().context("failed to load config")?;

    // 2. Initialise tracing
    init_tracing(&config);

    info!("sober-agent starting");

    // 3. Connect to PostgreSQL
    let db_config = sober_db::DatabaseConfig {
        url: config.database.url.clone(),
        max_connections: config.database.max_connections,
    };
    let pool = create_pool(&db_config)
        .await
        .context("failed to connect to PostgreSQL")?;

    // 4. Create repository instances
    let message_repo = Arc::new(PgMessageRepo::new(pool.clone()));
    let conversation_repo = Arc::new(PgConversationRepo::new(pool.clone()));
    let mcp_server_repo = Arc::new(PgMcpServerRepo::new(pool.clone()));

    // 5. Create LLM engine
    let llm: Arc<dyn sober_llm::LlmEngine> =
        Arc::new(OpenAiCompatibleEngine::from_config(&config.llm));

    // 6. Create memory store
    let memory = Arc::new(
        MemoryStore::new(&config.qdrant, config.llm.embedding_dim)
            .context("failed to create memory store")?,
    );

    // 7. Create context loader
    let context_loader = Arc::new(ContextLoader::new(
        Arc::clone(&memory),
        Arc::clone(&message_repo),
    ));

    // 8. Create Mind with SoulResolver
    let base_soul_path = std::env::current_dir()
        .unwrap_or_default()
        .join("soul")
        .join("SOUL.md");
    let user_soul_path = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join(".sõber").join("SOUL.md"));
    let workspace_soul_path: Option<PathBuf> = None;

    let soul_resolver = SoulResolver::new(base_soul_path, user_soul_path, workspace_soul_path);
    let mind = Arc::new(Mind::new(soul_resolver));

    // 9. Create tool registry with built-in tools
    //
    // The ShellTool uses a default workspace path and policy-based permission
    // mode. In production these will be derived from the workspace config per
    // conversation; for now we use sensible defaults.
    let workspace_home = PathBuf::from(
        std::env::var("WORKSPACE_ROOT")
            .unwrap_or_else(|_| "/var/lib/sober/workspaces/default".to_owned()),
    );
    let sandbox_profile = match std::env::var("SANDBOX_PROFILE").as_deref() {
        Ok("unrestricted") => SandboxProfile::Unrestricted,
        Ok("locked_down") => SandboxProfile::LockedDown,
        _ => SandboxProfile::Standard,
    };
    let sandbox_policy = sandbox_profile
        .resolve(&std::collections::HashMap::new())
        .context("failed to resolve sandbox policy")?;
    let command_policy = CommandPolicy::default();
    let shared_permission_mode = Arc::new(RwLock::new(PermissionMode::default()));
    let shell_tool = ShellTool::new(
        command_policy,
        Arc::clone(&shared_permission_mode),
        workspace_home,
        sandbox_policy,
        true, // auto_snapshot
    );

    let builtins: Vec<Arc<dyn Tool>> = vec![
        Arc::new(WebSearchTool::new(config.searxng.url.clone())),
        Arc::new(FetchUrlTool::new()),
        Arc::new(shell_tool),
    ];
    let tool_registry = Arc::new(ToolRegistry::with_builtins(builtins));

    // 10. Create confirmation broker
    let (mut confirmation_broker, confirmation_sender) = ConfirmationBroker::new();
    let registrar = confirmation_broker.registrar();

    // 11. Create Agent
    let agent_config = AgentConfig {
        model: config.llm.model.clone(),
        embedding_model: config.llm.embedding_model.clone(),
        max_tokens: config.llm.max_tokens,
        ..AgentConfig::default()
    };

    let agent = Arc::new(Agent::new(
        llm,
        mind,
        memory,
        context_loader,
        tool_registry,
        message_repo,
        conversation_repo,
        mcp_server_repo,
        agent_config,
        config.memory.clone(),
        Some(registrar),
    ));

    // 12. Spawn the confirmation broker loop
    tokio::spawn(async move { while confirmation_broker.process_next().await.is_some() {} });

    let grpc_service = AgentGrpcService::new(agent, confirmation_sender, shared_permission_mode);

    // 13. Bind to Unix domain socket
    let socket_path =
        std::env::var("AGENT_SOCKET_PATH").unwrap_or_else(|_| "/run/sober/agent.sock".to_owned());
    let socket_path = PathBuf::from(&socket_path);

    // Remove stale socket file if it exists
    if socket_path.exists() {
        std::fs::remove_file(&socket_path).with_context(|| {
            format!("failed to remove stale socket at {}", socket_path.display())
        })?;
    }

    // Ensure parent directory exists
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create socket directory {}", parent.display()))?;
    }

    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("failed to bind UDS at {}", socket_path.display()))?;
    let uds_stream = UnixListenerStream::new(listener);

    info!(socket = %socket_path.display(), "gRPC server listening");

    // 14. Serve with graceful shutdown
    Server::builder()
        .add_service(AgentServiceServer::new(grpc_service))
        .serve_with_incoming_shutdown(uds_stream, shutdown_signal())
        .await
        .context("gRPC server failed")?;

    info!("sober-agent shut down");
    Ok(())
}

/// Initialises the tracing subscriber based on the environment.
fn init_tracing(config: &AppConfig) {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("sober_agent=info,sober_mind=info,sober_memory=info"));

    match config.environment {
        sober_core::config::Environment::Production => {
            fmt().json().with_env_filter(filter).init();
        }
        sober_core::config::Environment::Development => {
            fmt().pretty().with_env_filter(filter).init();
        }
    }
}

/// Waits for SIGINT or SIGTERM for graceful shutdown.
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install SIGINT handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("received SIGINT"),
        () = terminate => info!("received SIGTERM"),
    }
}
