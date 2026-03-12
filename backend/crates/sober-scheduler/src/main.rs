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
use sober_db::{PgJobRepo, PgJobRunRepo, create_pool};
use sober_scheduler::engine::TickEngine;
use sober_scheduler::grpc::scheduler_proto::scheduler_service_server::SchedulerServiceServer;
use sober_scheduler::grpc::{CallerKeyStore, SchedulerGrpcService};
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
    let config = AppConfig::load_from_env().context("failed to load config")?;

    // 2. Initialise tracing
    init_tracing(&config);

    info!("sober-scheduler starting");

    // 3. Connect to PostgreSQL
    let db_config = sober_db::DatabaseConfig {
        url: config.database.url.clone(),
        max_connections: config.database.max_connections,
    };
    let pool = create_pool(&db_config)
        .await
        .context("failed to connect to PostgreSQL")?;

    // 4. Create repository instances
    let job_repo = Arc::new(PgJobRepo::new(pool.clone()));
    let run_repo = Arc::new(PgJobRunRepo::new(pool));

    // 5. Create tick engine
    let tick_interval = Duration::from_secs(config.scheduler.tick_interval_secs);
    let max_concurrent = config.scheduler.max_concurrent_jobs as usize;
    let engine = Arc::new(TickEngine::new(
        Arc::clone(&job_repo),
        Arc::clone(&run_repo),
        tick_interval,
        max_concurrent,
    ));

    // 6. Connect to agent gRPC service (background — retries until available)
    let agent_socket = config.scheduler.agent_socket_path.clone();
    let engine_for_agent = Arc::clone(&engine);
    tokio::spawn(async move {
        connect_to_agent(engine_for_agent, &agent_socket).await;
    });

    // 7. Create gRPC service with caller authentication
    //
    // CallerKeyStore starts empty — verification is skipped until keys are
    // registered. A future plan will add key exchange so services can
    // authenticate each other at startup.
    let caller_keys = Arc::new(CallerKeyStore::new());
    let grpc_service = SchedulerGrpcService::new(
        Arc::clone(&job_repo),
        Arc::clone(&run_repo),
        Arc::clone(&engine),
        caller_keys,
    );

    // 8. Bind to Unix domain socket
    let socket_path = config.scheduler.socket_path.clone();

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

    // 9. Start tick engine in background
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    let engine_for_tick = Arc::clone(&engine);
    tokio::spawn(async move {
        engine_for_tick.run(cancel_clone).await;
    });

    // 10. Serve gRPC with graceful shutdown
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

/// Initialises the tracing subscriber based on the environment.
fn init_tracing(config: &AppConfig) {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("sober_scheduler=info"));

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
