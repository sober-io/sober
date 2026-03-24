//! Sober Agent — gRPC server for agent orchestration, replica lifecycle, and
//! task delegation.
//!
//! This binary starts the agent gRPC service on a Unix domain socket. It is
//! called by `sober-api` and `sober-scheduler` at runtime.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use hyper_util::rt::TokioIo;
use sober_agent::ConfirmationBroker;
use sober_agent::SharedSchedulerClient;
use sober_agent::agent::{Agent, AgentConfig};
use sober_agent::backends::GrpcScheduleBackend;
use sober_agent::grpc::AgentGrpcService;
use sober_agent::grpc::proto::agent_service_server::AgentServiceServer;
use sober_agent::grpc::scheduler_proto;
use sober_agent::tools::{MemoryToolConfig, SearchToolConfig, ShellToolConfig, ToolBootstrap};
use sober_core::PermissionMode;
use sober_core::config::AppConfig;
use sober_crypto::envelope::Mek;
use sober_db::{PgAgentRepos, PgMessageRepo, PgPluginRepo, PgSandboxExecutionLogRepo, create_pool};
use sober_llm::OpenAiCompatibleEngine;
use sober_mcp::{McpConfig, McpPool};
use sober_memory::{ContextLoader, MemoryStore};
use sober_mind::assembly::Mind;
use sober_mind::soul::SoulResolver;
use sober_plugin::backends::{
    PgConversationBackend, PgKvBackend, PgSecretBackend, QdrantMemoryBackend,
};
use sober_plugin::{PluginManager, WasmServices};
use sober_sandbox::{CommandPolicy, SandboxProfile};
use sober_skill::SkillLoader;
use sober_workspace::BlobStore;
use std::sync::RwLock;
use tokio::net::UnixListener;
use tokio::signal;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::{Endpoint, Server, Uri};
use tower::service_fn;
use tracing::{info, warn};

/// TTL for the skill catalog cache. Skills are rescanned after this duration.
const SKILL_CACHE_TTL_SECS: u64 = 300;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Load .env and configuration
    let config = AppConfig::load().context("failed to load config")?;

    // 2. Initialise telemetry (tracing + metrics + OTel)
    let telemetry = sober_core::init_telemetry(
        config.environment,
        "sober_agent=info,sober_mind=info,sober_memory=info",
    );

    // 3. Spawn Prometheus metrics HTTP server for scraping
    sober_core::spawn_metrics_server(telemetry.prometheus.clone(), config.agent.metrics_port);

    info!("sober-agent starting");

    // 4. Connect to PostgreSQL
    let db_config = sober_db::DatabaseConfig {
        url: config.database.url.clone(),
        max_connections: config.database.max_connections,
    };
    let pool = create_pool(&db_config)
        .await
        .context("failed to connect to PostgreSQL")?;

    // 5. Create repository bundle
    let repos = Arc::new(PgAgentRepos::new(pool.clone()));

    // 6. Create LLM engine
    let llm: Arc<dyn sober_llm::LlmEngine> =
        Arc::new(OpenAiCompatibleEngine::from_config(&config.llm));

    // 7. Create memory store
    let memory = Arc::new(
        MemoryStore::new(&config.qdrant, config.llm.embedding_dim)
            .context("failed to create memory store")?,
    );

    // 8. Create context loader
    //
    // ContextLoader is generic over MessageRepo and needs its own Arc-wrapped
    // instance. PgPool is cheap to clone (internal Arc), so creating a second
    // PgMessageRepo is fine.
    let context_loader = Arc::new(ContextLoader::new(
        Arc::clone(&memory),
        Arc::new(PgMessageRepo::new(pool.clone())),
    ));

    // 9. Create Mind with SoulResolver
    //
    // Base soul.md is compiled into the binary — no filesystem path needed.
    // User layer: ~/.sober/soul.md (optional override).
    // Workspace layer: not set at startup (resolved per-conversation).
    let user_soul_path = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join(".sober").join("soul.md"));
    let user_instructions_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join(".sober"));

    let soul_resolver = SoulResolver::new(user_soul_path, None::<PathBuf>);
    let mind = Arc::new(
        Mind::new(soul_resolver, user_instructions_dir.as_deref())
            .context("failed to initialize mind")?,
    );

    // 10. Resolve workspace and sandbox configuration
    let workspace_root = config.agent.workspace_root.clone();
    let sandbox_profile = match config.agent.sandbox_profile.as_str() {
        "unrestricted" => SandboxProfile::Unrestricted,
        "locked_down" => SandboxProfile::LockedDown,
        _ => SandboxProfile::Standard,
    };
    let sandbox_policy = sandbox_profile
        .resolve(&std::collections::HashMap::new())
        .context("failed to resolve sandbox policy")?;
    let command_policy = CommandPolicy::default();
    let shared_permission_mode = Arc::new(RwLock::new(PermissionMode::default()));
    let snapshot_dir = workspace_root
        .join(sober_workspace::SOBER_DIR)
        .join("snapshots");
    let snapshot_manager = Arc::new(sober_workspace::SnapshotManager::new(snapshot_dir));

    // 11. Load optional MEK for secret encryption
    let mek: Option<Arc<Mek>> = config.crypto.master_encryption_key.as_ref().map(|hex| {
        let mek = Mek::from_hex(hex).expect("invalid MASTER_ENCRYPTION_KEY (need 64 hex chars)");
        info!("master encryption key loaded — secret tools enabled");
        Arc::new(mek)
    });
    if mek.is_none() {
        info!("no MASTER_ENCRYPTION_KEY set — secret tools disabled");
    }

    // 12. Create blob store for workspace artifacts
    let blob_store = Arc::new(BlobStore::new(
        workspace_root
            .join(sober_workspace::SOBER_DIR)
            .join("blobs"),
    ));

    // 13. Create shared scheduler client handle (connected in background later)
    let scheduler_client: SharedSchedulerClient = Arc::new(tokio::sync::RwLock::new(None));

    // 14. Create SkillLoader, PluginManager, and ToolBootstrap
    let skill_loader = Arc::new(SkillLoader::new(std::time::Duration::from_secs(
        SKILL_CACHE_TTL_SECS,
    )));

    let mcp_pool = McpPool::new(McpConfig::default());
    let plugin_repo = PgPluginRepo::new(pool.clone());
    let wasm_services = WasmServices {
        kv_backend: Some(Arc::new(PgKvBackend::new(pool.clone()))),
        llm_engine: Some(Arc::clone(&llm)),
        secret_backend: mek
            .as_ref()
            .map(|m| -> Arc<dyn sober_plugin::backends::SecretBackend> {
                Arc::new(PgSecretBackend::new(pool.clone(), Arc::clone(m)))
            }),
        memory_backend: Some(Arc::new(QdrantMemoryBackend::new(
            Arc::clone(&memory),
            Arc::clone(&llm),
        ))),
        conversation_backend: Some(Arc::new(PgConversationBackend::new(pool.clone()))),
        schedule_backend: Some(Arc::new(GrpcScheduleBackend::new(Arc::clone(
            &scheduler_client,
        )))),
        // ToolExecutor creates a circular dependency (ToolRegistry -> PluginManager
        // -> HostContext -> ToolExecutor -> ToolRegistry). Must be wired via a
        // post-init setter or lazy Arc once the tool registry is constructed.
        tool_executor: None,
        db_pool: Some(pool.clone()),
        system_prompt: {
            use sober_core::types::access::{CallerContext, TriggerKind};
            let caller = CallerContext {
                user_id: None,
                trigger: TriggerKind::Admin,
                permissions: vec![],
                scope_grants: vec![],
                workspace_id: None,
            };
            mind.base_system_prompt(&caller).await.ok()
        },
    };
    let plugin_manager = Arc::new(
        PluginManager::new(
            plugin_repo,
            mcp_pool,
            Arc::clone(&skill_loader),
            Some(Arc::clone(&blob_store)),
        )
        .with_wasm_services(wasm_services),
    );

    // Sync user-level skills from filesystem into the plugins table on startup.
    // This ensures ListSkills returns data immediately without waiting for a
    // conversation turn or manual reload.
    {
        let user_home = sober_workspace::user_home_dir();
        let _ = plugin_manager
            .sync_filesystem_skills(&user_home, &std::path::PathBuf::new(), None)
            .await;
        info!("startup skill sync complete");
    }

    let tool_bootstrap = Arc::new(ToolBootstrap {
        shell: ShellToolConfig {
            command_policy,
            permission_mode: Arc::clone(&shared_permission_mode),
            default_workspace_root: workspace_root,
            sandbox_policy,
            auto_snapshot: true,
            max_snapshots: None,
            sandbox_log_repo: Some(Arc::new(PgSandboxExecutionLogRepo::new(pool.clone()))),
        },
        search: SearchToolConfig {
            searxng_url: config.searxng.url.clone(),
        },
        memory_tools: MemoryToolConfig {
            memory: Arc::clone(&memory),
            llm: Arc::clone(&llm),
            config: config.memory.clone(),
        },
        scheduler_client: Arc::clone(&scheduler_client),
        repos: Arc::clone(&repos),
        mek: mek.clone(),
        blob_store,
        snapshot_manager,
        plugin_manager: Arc::clone(&plugin_manager),
        plugin_generator: Some(Arc::new(sober_plugin_gen::PluginGenerator::new(
            Arc::clone(&llm),
            config.llm.model.clone(),
        ))),
    });

    // 15. Create broadcast channel for conversation update events
    let (broadcast_tx, _broadcast_rx) = sober_agent::broadcast::create_broadcast_channel();

    // 16. Create confirmation broker
    let (mut confirmation_broker, confirmation_sender) = ConfirmationBroker::new();
    let registrar = confirmation_broker.registrar();

    // 17. Create Agent
    let agent_config = AgentConfig {
        model: config.llm.model.clone(),
        embedding_model: config.llm.embedding_model.clone(),
        max_tokens: config.llm.max_tokens,
        workspace_root: config.agent.workspace_root.clone(),
        ..AgentConfig::default()
    };

    let agent = Arc::new(Agent::new(
        llm,
        mind,
        memory,
        context_loader,
        repos,
        agent_config,
        config.memory.clone(),
        Some(registrar),
        broadcast_tx.clone(),
        mek,
        Some(config.llm.clone()),
        tool_bootstrap,
    ));

    // 18. Spawn the confirmation broker loop
    tokio::spawn(async move { while confirmation_broker.process_next().await.is_some() {} });

    // 19. Connect to scheduler gRPC service (background — retries until available)
    let scheduler_socket = config.scheduler.socket_path.clone();
    let scheduler_client_bg = Arc::clone(&scheduler_client);
    tokio::spawn(async move {
        connect_to_scheduler(scheduler_client_bg, &scheduler_socket).await;
    });

    let grpc_service = AgentGrpcService::new(
        agent,
        confirmation_sender,
        shared_permission_mode,
        broadcast_tx,
        plugin_manager,
    );

    // 20. Bind to Unix domain socket
    let socket_path = config.agent.socket_path.clone();

    // Remove stale socket file if it exists (best-effort).
    let _ = std::fs::remove_file(&socket_path);

    // Ensure parent directory exists (best-effort — under systemd,
    // RuntimeDirectory= creates it and ProtectSystem=strict blocks mkdir).
    if let Some(parent) = socket_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("failed to bind UDS at {}", socket_path.display()))?;
    let uds_stream = UnixListenerStream::new(listener);

    info!(socket = %socket_path.display(), "gRPC server listening");

    // 21. Serve with graceful shutdown
    Server::builder()
        .add_service(AgentServiceServer::new(grpc_service))
        .serve_with_incoming_shutdown(uds_stream, shutdown_signal())
        .await
        .context("gRPC server failed")?;

    info!("sober-agent shut down");
    Ok(())
}

/// Connect to the scheduler gRPC service and reconnect if the channel breaks.
///
/// Runs forever: establishes connection, monitors health, and re-establishes
/// on failure. This handles scheduler restarts gracefully.
async fn connect_to_scheduler(client: SharedSchedulerClient, socket_path: &Path) {
    use scheduler_proto::HealthRequest;

    let socket_path = socket_path.to_path_buf();

    loop {
        // Wait for the socket file to appear
        while !socket_path.exists() {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        let path_clone = socket_path.clone();
        let channel = Endpoint::try_from("http://[::]:50051")
            .expect("static URI is valid")
            .connect_with_connector(service_fn(move |_: Uri| {
                let path = path_clone.clone();
                async move {
                    let stream = tokio::net::UnixStream::connect(path).await?;
                    Ok::<_, std::io::Error>(TokioIo::new(stream))
                }
            }))
            .await;

        match channel {
            Ok(channel) => {
                let sched_client =
                    scheduler_proto::scheduler_service_client::SchedulerServiceClient::new(channel);
                {
                    let mut lock = client.write().await;
                    *lock = Some(sched_client.clone());
                }
                info!(
                    socket = %socket_path.display(),
                    "connected to scheduler gRPC service"
                );

                // Register predefined system jobs idempotently
                sober_agent::system_jobs::register_system_jobs(&client).await;

                // Monitor connection health
                let mut sched_client = sched_client;
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    match sched_client.health(HealthRequest {}).await {
                        Ok(_) => {}
                        Err(e) => {
                            warn!(error = %e, "scheduler connection lost, reconnecting");
                            let mut lock = client.write().await;
                            *lock = None;
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                warn!(
                    error = %e,
                    socket = %socket_path.display(),
                    "failed to connect to scheduler, retrying in 5s"
                );
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
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
