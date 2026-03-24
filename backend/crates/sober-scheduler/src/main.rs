//! Sober Scheduler — autonomous tick engine with interval and cron scheduling,
//! job persistence, and admin socket.
//!
//! This binary starts the scheduler tick engine and gRPC admin service on a Unix
//! domain socket. It connects to the agent via gRPC/UDS to dispatch job
//! executions and wake notifications.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use hyper_util::rt::TokioIo;
use sober_core::config::AppConfig;
use sober_db::{PgJobRepo, PgJobRunRepo, PgSessionRepo, create_pool};
use sober_memory::MemoryStore;
use sober_sandbox::SandboxProfile;
use sober_scheduler::engine::TickEngine;
use sober_scheduler::executor::JobExecutorRegistry;
use sober_scheduler::executors::artifact::ArtifactExecutor;
use sober_scheduler::executors::memory_pruning::MemoryPruningExecutor;
use sober_scheduler::executors::plugin_cleanup::PluginCleanupExecutor;
use sober_scheduler::executors::session_cleanup::SessionCleanupExecutor;
use sober_scheduler::grpc::SchedulerGrpcService;
use sober_scheduler::grpc::scheduler_proto::scheduler_service_server::SchedulerServiceServer;
use sober_scheduler::system_jobs::register_system_jobs;
use sober_workspace::BlobStore;
use tokio::net::UnixListener;
use tokio::signal;
use tokio_stream::wrappers::UnixListenerStream;
use tokio_util::sync::CancellationToken;
use tonic::transport::{Endpoint, Server, Uri};
use tower::service_fn;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Load .env and configuration
    let config = AppConfig::load().context("failed to load config")?;

    // 2. Initialise telemetry (tracing + metrics + OTel)
    let telemetry = sober_core::init_telemetry(config.environment, "sober_scheduler=info");

    // 3. Spawn Prometheus metrics HTTP server for scraping
    sober_core::spawn_metrics_server(telemetry.prometheus.clone(), config.scheduler.metrics_port);

    info!("sober-scheduler starting");

    // 4. Connect to PostgreSQL
    let db_config = sober_db::DatabaseConfig {
        url: config.database.url.clone(),
        max_connections: config.database.max_connections,
    };
    let pool = create_pool(&db_config)
        .await
        .context("failed to connect to PostgreSQL")?;

    // 5. Create repository instances
    let job_repo = Arc::new(PgJobRepo::new(pool.clone()));
    let run_repo = Arc::new(PgJobRunRepo::new(pool.clone()));
    let session_repo = PgSessionRepo::new(pool.clone());

    // 6. Build job executor registry
    let executor_registry = build_executor_registry(&config, session_repo, pool)?;
    let executor_registry = Arc::new(executor_registry);

    // 7. Register system maintenance jobs
    if let Err(e) = register_system_jobs(job_repo.as_ref()).await {
        warn!(error = %e, "failed to register system jobs (will retry next startup)");
    }

    // 8. Create tick engine
    let tick_interval = Duration::from_secs(config.scheduler.tick_interval_secs);
    let max_concurrent = config.scheduler.max_concurrent_jobs as usize;
    let engine = Arc::new(TickEngine::new(
        Arc::clone(&job_repo),
        Arc::clone(&run_repo),
        Arc::clone(&executor_registry),
        tick_interval,
        max_concurrent,
    ));

    // 9. Connect to agent gRPC service (background — retries until available)
    let agent_socket = config.scheduler.agent_socket_path.clone();
    let engine_for_agent = Arc::clone(&engine);
    tokio::spawn(async move {
        connect_to_agent(engine_for_agent, &agent_socket).await;
    });

    // 10. Create gRPC service
    let grpc_service = SchedulerGrpcService::new(
        Arc::clone(&job_repo),
        Arc::clone(&run_repo),
        Arc::clone(&engine),
    );

    // 11. Bind to Unix domain socket
    let socket_path = config.scheduler.socket_path.clone();

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

    // 12. Start tick engine in background
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    let engine_for_tick = Arc::clone(&engine);
    tokio::spawn(async move {
        engine_for_tick.run(cancel_clone).await;
    });

    // 13. Serve gRPC with graceful shutdown
    Server::builder()
        .add_service(SchedulerServiceServer::new(grpc_service))
        .serve_with_incoming_shutdown(uds_stream, shutdown_signal())
        .await
        .context("gRPC server failed")?;

    // Signal the tick engine to stop
    cancel.cancel();

    info!("sober-scheduler shut down");
    Ok(())
}

/// Build the job executor registry with all concrete executors.
fn build_executor_registry(
    config: &AppConfig,
    session_repo: PgSessionRepo,
    pool: sqlx::PgPool,
) -> Result<JobExecutorRegistry> {
    let mut registry = JobExecutorRegistry::new();

    // Memory pruning executor
    let memory_store = Arc::new(
        MemoryStore::new(&config.qdrant, config.llm.embedding_dim)
            .context("failed to create memory store for scheduler")?,
    );
    registry.register(
        "memory_pruning",
        Arc::new(MemoryPruningExecutor::new(
            memory_store,
            config.memory.clone(),
        )),
    );

    // Session cleanup executor
    registry.register(
        "session_cleanup",
        Arc::new(SessionCleanupExecutor::new(session_repo)),
    );

    // Artifact executor
    let blob_root = config
        .scheduler
        .workspace_root
        .join(sober_workspace::SOBER_DIR)
        .join("blobs");
    let blob_store = Arc::new(BlobStore::new(blob_root));

    let sandbox_profile = match config.scheduler.sandbox_profile.as_str() {
        "unrestricted" => SandboxProfile::Unrestricted,
        "locked_down" => SandboxProfile::LockedDown,
        _ => SandboxProfile::Standard,
    };
    let sandbox_policy = sandbox_profile
        .resolve(&std::collections::HashMap::new())
        .context("failed to resolve sandbox policy")?;

    // Plugin cleanup executor
    let cleanup_plugin_repo = sober_db::PgPluginRepo::new(pool.clone());
    registry.register(
        "plugin_cleanup",
        Arc::new(PluginCleanupExecutor::new(cleanup_plugin_repo)),
    );

    let sandbox_log_repo = Arc::new(sober_db::PgSandboxExecutionLogRepo::new(pool));
    registry.register(
        "artifact",
        Arc::new(ArtifactExecutor::new(
            blob_store,
            sandbox_policy,
            Some(sandbox_log_repo),
        )),
    );

    Ok(registry)
}

/// Connect to the agent gRPC service and reconnect if the channel breaks.
///
/// Runs forever: establishes connection, monitors health, and re-establishes
/// on failure. This handles agent restarts gracefully.
async fn connect_to_agent(engine: Arc<TickEngine<PgJobRepo, PgJobRunRepo>>, socket_path: &Path) {
    use sober_scheduler::grpc::agent_proto::HealthRequest;
    use sober_scheduler::grpc::agent_proto::agent_service_client::AgentServiceClient;

    let socket_path = socket_path.to_path_buf();

    loop {
        // Wait for the socket file to appear
        while !socket_path.exists() {
            tokio::time::sleep(Duration::from_secs(2)).await;
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
                let client = AgentServiceClient::new(channel);
                engine.set_agent_client(client.clone()).await;
                info!(
                    socket = %socket_path.display(),
                    "connected to agent gRPC service"
                );

                // Monitor connection health
                let mut client = client;
                loop {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    match client.health(HealthRequest {}).await {
                        Ok(_) => {} // still healthy
                        Err(e) => {
                            warn!(error = %e, "agent connection lost, reconnecting");
                            engine.clear_agent_client().await;
                            break; // re-enter outer loop
                        }
                    }
                }
            }
            Err(e) => {
                warn!(
                    error = %e,
                    socket = %socket_path.display(),
                    "failed to connect to agent, retrying in 5s"
                );
                tokio::time::sleep(Duration::from_secs(5)).await;
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
