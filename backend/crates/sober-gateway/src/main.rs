//! Sober Gateway — external messaging platform bridge.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use hyper_util::rt::TokioIo;
use sober_core::config::AppConfig;
use sober_db::create_pool;
use sober_gateway::agent_proto::agent_service_client::AgentServiceClient;
use sober_gateway::bridge::PlatformBridgeRegistry;
use sober_gateway::grpc::GatewayGrpcService;
use sober_gateway::outbound::OutboundBuffer;
use sober_gateway::proto::gateway_service_server::GatewayServiceServer;
use sober_gateway::service::GatewayService;
use sober_gateway::types::GatewayEvent;
use tokio::net::UnixListener;
use tokio::sync::mpsc;
use tokio::{signal, time};
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::{Endpoint, Server, Uri};
use tower::service_fn;
use tracing::{error, info, warn};

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

    // Set up bridge registry and gateway service.
    let bridge_registry = Arc::new(PlatformBridgeRegistry::new());
    let service = Arc::new(GatewayService::new(
        pool,
        agent_client,
        bridge_registry.clone(),
    ));

    service
        .load_caches()
        .await
        .context("failed to load gateway caches")?;

    // Inbound event channel.
    // event_tx is cloned and passed to each bridge when platforms connect at runtime.
    let (event_tx, mut event_rx) = mpsc::channel::<GatewayEvent>(1024);
    // Keep event_tx alive so the channel stays open for the inbound loop.
    let _event_tx = event_tx;

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
            run_outbound_loop(service, agent_channel_for_sub).await;
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
        .serve_with_incoming_shutdown(stream, shutdown_signal())
        .await?;

    info!("sober-gateway stopped");
    Ok(())
}

/// Subscribes to the agent's conversation update stream and delivers
/// completed responses to mapped external platform channels.
async fn run_outbound_loop(service: Arc<GatewayService>, agent_channel: tonic::transport::Channel) {
    let mut backoff = time::Duration::from_secs(1);

    loop {
        info!("starting outbound delivery loop");

        match run_outbound_stream(service.clone(), agent_channel.clone()).await {
            Ok(()) => {
                info!("outbound stream ended, reconnecting");
            }
            Err(e) => {
                error!(error = %e, "outbound stream error, reconnecting after backoff");
            }
        }

        time::sleep(backoff).await;
        backoff = (backoff * 2).min(time::Duration::from_secs(30));
    }
}

/// Runs one session of the outbound stream until it ends or errors.
async fn run_outbound_stream(
    service: Arc<GatewayService>,
    agent_channel: tonic::transport::Channel,
) -> Result<()> {
    use sober_gateway::agent_proto::{
        SubscribeRequest, agent_service_client::AgentServiceClient, conversation_update::Event,
    };

    let mut client = AgentServiceClient::new(agent_channel);
    let mut stream = client
        .subscribe_conversation_updates(SubscribeRequest {})
        .await
        .context("failed to subscribe to conversation updates")?
        .into_inner();

    let mut buffer = OutboundBuffer::new();

    while let Some(update) = stream
        .message()
        .await
        .context("error reading conversation update stream")?
    {
        let conversation_id_str = &update.conversation_id;

        let conversation_id = match conversation_id_str.parse::<uuid::Uuid>() {
            Ok(uuid) => sober_core::types::ConversationId::from_uuid(uuid),
            Err(_) => {
                warn!(conversation_id = %conversation_id_str, "invalid conversation_id in update");
                continue;
            }
        };

        match update.event {
            Some(Event::TextDelta(delta)) => {
                buffer.append_delta(conversation_id, &delta.content);
            }
            Some(Event::Done(_)) => {
                if let Some(msg) = buffer.flush(&conversation_id) {
                    deliver_outbound(service.as_ref(), conversation_id, msg).await;
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// Delivers an outbound message to all mapped external channels for a conversation.
async fn deliver_outbound(
    service: &GatewayService,
    conversation_id: sober_core::types::ConversationId,
    msg: sober_gateway::types::PlatformMessage,
) {
    let targets = service.get_outbound_targets(&conversation_id);

    if targets.is_empty() {
        return;
    }

    for (platform_id, channel_id) in targets {
        if let Some(bridge) = service.bridge_registry().get(&platform_id) {
            if let Err(e) = bridge.send_message(&channel_id, msg.clone()).await {
                error!(
                    error = %e,
                    platform_id = %platform_id,
                    channel_id = %channel_id,
                    "failed to deliver outbound message"
                );
                metrics::counter!("gateway_outbound_errors_total").increment(1);
            } else {
                metrics::counter!("gateway_outbound_messages_total").increment(1);
            }
        }
    }
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
