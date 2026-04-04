//! Discord client bridge implementation using Serenity.

use std::sync::Arc;

use serenity::http::Http;
use serenity::model::channel::ChannelType;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::{error, info};

use sober_core::types::{PlatformId, PlatformType};

use crate::bridge::PlatformBridgeHandle;
use crate::error::GatewayError;
use crate::outbound::split_message;
use crate::types::{ExternalChannel, GatewayEvent, PlatformMessage};

use super::handler::DiscordHandler;

/// Discord's message character limit.
const DISCORD_MAX_LEN: usize = 2000;

/// Discord platform bridge.
///
/// Wraps a serenity client, connecting to Discord's gateway and providing
/// send/list operations via the HTTP API.
pub struct DiscordBridge {
    #[allow(dead_code)]
    platform_id: PlatformId,
    http: Arc<Http>,
    /// Shutdown signal sender — send any value to stop the serenity client.
    shutdown_tx: Mutex<Option<mpsc::Sender<()>>>,
}

impl DiscordBridge {
    /// Creates and connects a Discord bridge, spawning the serenity client
    /// in a background task.
    ///
    /// Returns the bridge handle and starts receiving messages into `event_tx`.
    pub async fn connect(
        platform_id: PlatformId,
        bot_token: &str,
        event_tx: mpsc::Sender<GatewayEvent>,
    ) -> Result<Arc<Self>, GatewayError> {
        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT
            | GatewayIntents::GUILDS;

        let http = Arc::new(Http::new(bot_token));

        let handler = DiscordHandler {
            platform_id,
            event_tx,
            bot_user_id: Mutex::new(None),
        };

        let mut client = Client::builder(bot_token, intents)
            .event_handler(handler)
            .await
            .map_err(|e| GatewayError::ConnectionFailed(e.to_string()))?;

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

        // Move the shard manager out so we can stop the client on shutdown.
        let shard_manager = client.shard_manager.clone();

        tokio::spawn(async move {
            tokio::select! {
                result = client.start() => {
                    if let Err(e) = result {
                        error!(error = %e, "Discord client error");
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Discord bridge shutdown requested");
                    shard_manager.shutdown_all().await;
                }
            }
        });

        info!(platform_id = %platform_id, "Discord bridge started");

        Ok(Arc::new(Self {
            platform_id,
            http,
            shutdown_tx: Mutex::new(Some(shutdown_tx)),
        }))
    }

    /// Sends a shutdown signal to the running serenity client.
    pub async fn disconnect(&self) {
        if let Some(tx) = self.shutdown_tx.lock().await.take() {
            let _ = tx.send(()).await;
        }
    }
}

#[async_trait::async_trait]
impl PlatformBridgeHandle for DiscordBridge {
    async fn send_message(
        &self,
        channel_id: &str,
        content: PlatformMessage,
    ) -> Result<(), GatewayError> {
        let channel_id: u64 = channel_id.parse().map_err(|_| {
            GatewayError::ChannelNotFound(format!("invalid Discord channel ID: {channel_id}"))
        })?;

        let channel = ChannelId::new(channel_id);

        // Discord has a 2000-character limit per message. Split at natural
        // boundaries when possible to avoid breaking markdown.
        for chunk in split_message(&content.text, DISCORD_MAX_LEN) {
            channel
                .say(&self.http, chunk)
                .await
                .map_err(|e| GatewayError::SendFailed(e.to_string()))?;
        }

        Ok(())
    }

    async fn list_channels(&self) -> Result<Vec<ExternalChannel>, GatewayError> {
        let guilds = self
            .http
            .get_guilds(None, None)
            .await
            .map_err(|e| GatewayError::ConnectionFailed(e.to_string()))?;

        let mut channels = Vec::new();

        for guild_info in guilds {
            let guild_channels = self
                .http
                .get_channels(guild_info.id)
                .await
                .map_err(|e| GatewayError::ConnectionFailed(e.to_string()))?;

            for ch in guild_channels {
                if ch.kind == ChannelType::Text || ch.kind == ChannelType::PublicThread {
                    let kind = match ch.kind {
                        ChannelType::Text => "text",
                        ChannelType::PublicThread => "thread",
                        _ => "other",
                    };
                    channels.push(ExternalChannel {
                        id: ch.id.to_string(),
                        name: ch.name,
                        kind: kind.to_string(),
                    });
                }
            }
        }

        Ok(channels)
    }

    async fn start_typing(&self, channel_id: &str) -> Result<(), GatewayError> {
        let channel_id: u64 = channel_id.parse().map_err(|_| {
            GatewayError::ChannelNotFound(format!("invalid Discord channel ID: {channel_id}"))
        })?;
        let channel = ChannelId::new(channel_id);
        channel
            .broadcast_typing(&self.http)
            .await
            .map_err(|e| GatewayError::SendFailed(e.to_string()))?;
        Ok(())
    }

    fn platform_type(&self) -> PlatformType {
        PlatformType::Discord
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_message_is_single_chunk() {
        let chunks = split_message("hello", DISCORD_MAX_LEN);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn splits_at_newline_boundary() {
        let line = "a".repeat(1500);
        let text = format!("{line}\n{line}");
        let chunks = split_message(&text, DISCORD_MAX_LEN);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].len() <= DISCORD_MAX_LEN);
        assert!(chunks[1].len() <= DISCORD_MAX_LEN);
    }

    #[test]
    fn splits_at_sentence_boundary() {
        // No newlines, but has sentence endings
        let sentence = "Hello world. ";
        let count = DISCORD_MAX_LEN / sentence.len() + 5;
        let text = sentence.repeat(count);
        let chunks = split_message(&text, DISCORD_MAX_LEN);
        assert!(chunks.len() >= 2);
        // First chunk should end at a sentence boundary
        assert!(chunks[0].ends_with(". "));
    }

    #[test]
    fn splits_at_word_boundary() {
        // No newlines, no sentence endings, but has spaces
        let text = "word ".repeat(500);
        let chunks = split_message(&text, DISCORD_MAX_LEN);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].ends_with(' '));
    }

    #[test]
    fn hard_splits_without_any_boundaries() {
        let text = "a".repeat(3000);
        let chunks = split_message(&text, DISCORD_MAX_LEN);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), DISCORD_MAX_LEN);
        assert_eq!(chunks[1].len(), 1000);
    }
}
