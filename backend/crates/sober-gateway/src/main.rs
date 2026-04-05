//! Sober Gateway — external messaging platform bridge.

mod helpers;

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use hyper_util::rt::TokioIo;
use sober_core::config::AppConfig;
use sober_db::create_pool;
use sober_gateway::agent_proto::agent_service_client::AgentServiceClient;
use sober_gateway::bridge::PlatformBridgeRegistry;
use sober_gateway::grpc::GatewayGrpcService;
use sober_gateway::proto::gateway_service_server::GatewayServiceServer;
use sober_gateway::service::GatewayService;
use sober_gateway::types::GatewayEvent;
use sober_workspace::BlobStore;
use tokio::net::UnixListener;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::{Endpoint, Server, Uri};
use tower::service_fn;
use tracing::{error, info};

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
    let pool = create_pool(&db_config)
        .await
        .context("failed to connect to PostgreSQL")?;

    // Connect to agent gRPC.
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

    let agent_client = AgentServiceClient::new(agent_channel.clone());

    info!("connected to agent gRPC service");

    // Inbound event channel.
    // event_tx is cloned and passed to each bridge when platforms connect.
    let (event_tx, mut event_rx) = mpsc::channel::<GatewayEvent>(1024);

    // Blob store for direct attachment storage/retrieval.
    let blob_root = config
        .workspace_root
        .join(sober_workspace::SOBER_DIR)
        .join("blobs");
    let blob_store = Arc::new(BlobStore::new(blob_root));

    // Set up bridge registry and gateway service.
    let bridge_registry = Arc::new(PlatformBridgeRegistry::new());
    let service = Arc::new(GatewayService::new(
        pool,
        agent_client,
        bridge_registry.clone(),
        event_tx.clone(),
        blob_store,
    ));

    service
        .load_caches()
        .await
        .context("failed to load gateway caches")?;

    // Connect enabled platforms from the database.
    if let Err(e) = service.connect_platforms().await {
        error!(error = %e, "failed to connect platforms at startup (will retry on Reload)");
    }

    // Spawn inbound event loop.
    {
        let service = service.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                service.handle_event(event).await;
            }
            info!("inbound event loop stopped");
        });
    }

    // Spawn outbound delivery loop.
    {
        let service = service.clone();
        let agent_channel_for_sub = agent_channel;
        tokio::spawn(async move {
            helpers::run_outbound_loop(service, agent_channel_for_sub).await;
        });
    }

    // Clean up stale socket.
    let socket_path = &config.gateway.socket_path;
    if Path::new(socket_path).exists() {
        std::fs::remove_file(socket_path)?;
    }

    let listener = UnixListener::bind(socket_path)?;
    let stream = UnixListenerStream::new(listener);

    info!(socket = %socket_path.display(), "gRPC server listening");

    let grpc_service = GatewayGrpcService::new(service);

    Server::builder()
        .add_service(GatewayServiceServer::new(grpc_service))
        .serve_with_incoming_shutdown(stream, helpers::shutdown_signal())
        .await?;

    info!("sober-gateway stopped");
    Ok(())
}
