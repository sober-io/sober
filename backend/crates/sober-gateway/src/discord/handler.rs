//! Serenity event handler for the Discord bridge.

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::UserId as DiscordUserId;
use serenity::prelude::*;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use sober_core::types::PlatformId;

use crate::types::{GatewayEvent, InboundAttachment};

/// Serenity event handler that receives Discord events and forwards them
/// into the gateway event channel.
pub struct DiscordHandler {
    /// The platform ID registered for this Discord connection.
    pub platform_id: PlatformId,
    /// Channel to forward gateway events into.
    pub event_tx: mpsc::Sender<GatewayEvent>,
    /// The bot's own Discord user ID, set on Ready.
    pub bot_user_id: Mutex<Option<DiscordUserId>>,
}

#[async_trait]
impl EventHandler for DiscordHandler {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        let bot_id = ready.user.id;
        *self.bot_user_id.lock().await = Some(bot_id);
        info!(
            platform_id = %self.platform_id,
            bot_user = %ready.user.name,
            "Discord bot connected"
        );
    }

    async fn message(&self, _ctx: Context, msg: Message) {
        // Skip messages from bots (including ourselves).
        if msg.author.bot {
            return;
        }

        // Guard: skip if the bot user ID hasn't been set yet.
        let bot_id = *self.bot_user_id.lock().await;
        if let Some(bot_id) = bot_id
            && msg.author.id == bot_id
        {
            return;
        }

        debug!(
            platform_id = %self.platform_id,
            channel_id = %msg.channel_id,
            author = %msg.author.name,
            attachment_count = msg.attachments.len(),
            "received Discord message"
        );

        // Download attachments from Discord CDN.
        let mut attachments = Vec::with_capacity(msg.attachments.len());
        for attachment in &msg.attachments {
            let start = std::time::Instant::now();
            match download_attachment(&attachment.url, &attachment.filename).await {
                Ok(inbound) => {
                    metrics::counter!(
                        "sober_gateway_attachments_downloaded_total",
                        "platform" => "discord",
                        "status" => "success",
                    )
                    .increment(1);
                    metrics::histogram!(
                        "sober_gateway_attachment_download_duration_seconds",
                        "platform" => "discord",
                    )
                    .record(start.elapsed().as_secs_f64());
                    metrics::histogram!(
                        "sober_gateway_attachment_download_bytes",
                        "platform" => "discord",
                    )
                    .record(inbound.data.len() as f64);
                    attachments.push(inbound);
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        filename = %attachment.filename,
                        url = %attachment.url,
                        "failed to download Discord attachment, skipping"
                    );
                    metrics::counter!(
                        "sober_gateway_attachments_downloaded_total",
                        "platform" => "discord",
                        "status" => "error",
                    )
                    .increment(1);
                }
            }
        }

        let event = GatewayEvent::MessageReceived {
            platform_id: self.platform_id,
            channel_id: msg.channel_id.to_string(),
            user_id: msg.author.id.to_string(),
            username: msg.author.name.clone(),
            content: msg.content.clone(),
            attachments,
        };

        if let Err(e) = self.event_tx.send(event).await {
            tracing::error!(error = %e, "failed to forward Discord message to event loop");
        }
    }
}

/// Downloads an attachment from a platform CDN URL.
async fn download_attachment(url: &str, filename: &str) -> Result<InboundAttachment, String> {
    let response = reqwest::Client::new()
        .get(url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());

    let data = response
        .bytes()
        .await
        .map_err(|e| format!("failed to read body: {e}"))?
        .to_vec();

    Ok(InboundAttachment {
        filename: filename.to_owned(),
        content_type,
        data,
    })
}
