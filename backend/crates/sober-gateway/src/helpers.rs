//! Helper functions for the gateway binary: outbound delivery loop and signal handling.

use std::sync::Arc;

use anyhow::{Context, Result};
use sober_gateway::outbound::OutboundBuffer;
use sober_gateway::service::GatewayService;
use tracing::{error, info, warn};

/// Subscribes to the agent's conversation update stream and delivers
/// completed responses to mapped external platform channels.
///
/// Reconnects with exponential backoff on stream errors.
pub async fn run_outbound_loop(
    service: Arc<GatewayService>,
    agent_channel: tonic::transport::Channel,
) {
    let mut backoff = tokio::time::Duration::from_secs(1);

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

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(tokio::time::Duration::from_secs(30));
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
                metrics::counter!("sober_gateway_platform_errors_total", "platform" => "unknown", "error_type" => "sdk").increment(1);
            } else {
                metrics::counter!("sober_gateway_messages_sent_total", "platform" => "unknown", "status" => "success").increment(1);
            }
        }
    }
}

/// Waits for SIGTERM or Ctrl-C.
pub async fn shutdown_signal() {
    let sigterm = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    tokio::select! {
        () = sigterm => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}
