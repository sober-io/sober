//! Serenity event handler for the Discord bridge.

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::UserId as DiscordUserId;
use serenity::prelude::*;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, info};

use sober_core::types::PlatformId;

use crate::types::GatewayEvent;

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
            "received Discord message"
        );

        let event = GatewayEvent::MessageReceived {
            platform_id: self.platform_id,
            channel_id: msg.channel_id.to_string(),
            user_id: msg.author.id.to_string(),
            username: msg.author.name.clone(),
            content: msg.content.clone(),
        };

        if let Err(e) = self.event_tx.send(event).await {
            tracing::error!(error = %e, "failed to forward Discord message to event loop");
        }
    }
}
