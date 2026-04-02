//! Sober Gateway — external messaging platform bridge.

use std::path::Path;

use anyhow::{Context, Result};
use hyper_util::rt::TokioIo;
use sober_core::config::AppConfig;
use sober_db::create_pool;
use sober_gateway::grpc::GatewayGrpcService;
use sober_gateway::proto::gateway_service_server::GatewayServiceServer;
use tokio::net::UnixListener;
use tokio::signal;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::{Endpoint, Server, Uri};
use tower::service_fn;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let config = AppConfig::load().context("failed to load config")?;

    let telemetry = sober_core::init_telemetry(
        config.environment,
        "sober_gateway=info,sqlx::query=warn,info",
    );

    sober_core::spawn_metrics_server(telemetry.prometheus.clone(), config.gateway.metrics_port);

    info!("sober-gateway starting");

    let db_config = sober_db::DatabaseConfig {
        url: config.database.url.clone(),
        max_connections: config.database.max_connections,
    };
    let _pool = create_pool(&db_config)
        .await
        .context("failed to connect to PostgreSQL")?;

    // Connect to agent gRPC
    let agent_socket_path = config.gateway.agent_socket_path.clone();
    let agent_channel = Endpoint::try_from("http://[::]:50051")?
        .connect_with_connector(service_fn(move |_: Uri| {
            let path = agent_socket_path.clone();
            async move {
                let stream = tokio::net::UnixStream::connect(path).await?;
                Ok::<_, std::io::Error>(TokioIo::new(stream))
            }
        }))
        .await
        .context("failed to connect to agent gRPC")?;
    let _agent_client =
        sober_gateway::agent_proto::agent_service_client::AgentServiceClient::new(agent_channel);

    info!("connected to agent gRPC service");

    // Clean up stale socket
    let socket_path = &config.gateway.socket_path;
    if Path::new(socket_path).exists() {
        std::fs::remove_file(socket_path)?;
    }

    let listener = UnixListener::bind(socket_path)?;
    let stream = UnixListenerStream::new(listener);

    info!(socket = %socket_path.display(), "gRPC server listening");

    let grpc_service = GatewayGrpcService;

    Server::builder()
        .add_service(GatewayServiceServer::new(grpc_service))
        .serve_with_incoming_shutdown(stream, shutdown_signal())
        .await?;

    info!("sober-gateway stopped");
    Ok(())
}

async fn shutdown_signal() {
    let sigterm = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    tokio::select! {
        () = sigterm => {}
        _ = signal::ctrl_c() => {}
    }
}
