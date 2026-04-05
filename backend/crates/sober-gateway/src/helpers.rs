//! Helper functions for the gateway binary: outbound delivery loop and signal handling.

use std::sync::Arc;

use anyhow::{Context, Result};
use sober_gateway::outbound::OutboundBuffer;
use sober_gateway::service::GatewayService;
use tracing::{debug, error, info, warn};

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
    let mut typing_sent: std::collections::HashSet<sober_core::types::ConversationId> =
        std::collections::HashSet::new();

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
            Some(Event::NewMessage(ref nm)) if nm.role.to_lowercase() == "user" => {
                // Skip messages that originated from this gateway to avoid echo.
                if nm.source == "gateway" {
                    continue;
                }

                // Forward user messages from the web UI to external platforms with a sender prefix.
                let text: String = nm
                    .content
                    .iter()
                    .filter_map(|b| {
                        use sober_gateway::agent_proto::content_block::Block;
                        match b.block.as_ref()? {
                            Block::Text(t) => Some(t.text.as_str()),
                            _ => None,
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                if !text.is_empty() {
                    // Resolve the user ID to a username for a friendlier prefix.
                    let user_label = if let Some(ref uid_str) = nm.user_id {
                        if let Ok(uuid) = uid_str.parse::<uuid::Uuid>() {
                            let uid = sober_core::types::UserId::from_uuid(uuid);
                            service
                                .resolve_username(&uid)
                                .await
                                .unwrap_or_else(|| uid_str.clone())
                        } else {
                            "web".to_owned()
                        }
                    } else {
                        "web".to_owned()
                    };
                    let prefixed = format!("**[{user_label}]** {text}");
                    let msg = sober_gateway::types::PlatformMessage {
                        text: prefixed,
                        format: sober_gateway::types::MessageFormat::Markdown,
                        reply_to: None,
                    };
                    deliver_outbound(service.as_ref(), conversation_id, msg).await;
                }
            }
            Some(Event::TextDelta(delta)) => {
                // Trigger typing indicator on first delta for this response.
                if typing_sent.insert(conversation_id) {
                    trigger_typing(service.as_ref(), conversation_id).await;
                }
                buffer.append_delta(conversation_id, &delta.content);
            }
            Some(Event::Done(_)) => {
                typing_sent.remove(&conversation_id);
                if let Some(msg) = buffer.flush(&conversation_id) {
                    debug!(
                        conversation_id = %conversation_id_str,
                        text_len = msg.text.len(),
                        "flushing outbound buffer"
                    );
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

    debug!(conversation_id = %conversation_id, target_count = targets.len(), "delivering to platforms");

    for (platform_id, channel_id) in targets {
        if let Some(bridge) = service.bridge_registry().get(&platform_id) {
            let platform_label = bridge.platform_type().to_string();
            let start = std::time::Instant::now();
            if let Err(e) = bridge.send_message(&channel_id, msg.clone()).await {
                error!(
                    error = %e,
                    platform_id = %platform_id,
                    channel_id = %channel_id,
                    "failed to deliver outbound message"
                );
                metrics::counter!("sober_gateway_platform_errors_total", "platform" => platform_label.clone(), "error_type" => "sdk").increment(1);
                metrics::counter!("sober_gateway_messages_sent_total", "platform" => platform_label.clone(), "status" => "error").increment(1);
            } else {
                metrics::counter!("sober_gateway_messages_sent_total", "platform" => platform_label.clone(), "status" => "success").increment(1);
            }
            metrics::histogram!("sober_gateway_message_delivery_duration_seconds", "platform" => platform_label).record(start.elapsed().as_secs_f64());
        }
    }
}

/// Sends typing indicators to all mapped platform channels for a conversation.
async fn trigger_typing(
    service: &GatewayService,
    conversation_id: sober_core::types::ConversationId,
) {
    let targets = service.get_outbound_targets(&conversation_id);
    for (platform_id, channel_id) in targets {
        if let Some(bridge) = service.bridge_registry().get(&platform_id)
            && let Err(e) = bridge.start_typing(&channel_id).await
        {
            tracing::debug!(error = %e, "failed to send typing indicator");
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
