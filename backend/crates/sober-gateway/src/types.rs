//! Gateway-specific types for events and messages.

use sober_core::types::PlatformId;

/// An attachment downloaded from an external platform.
#[derive(Debug)]
pub struct InboundAttachment {
    /// Original filename from the platform.
    pub filename: String,
    /// MIME content type reported by the platform.
    pub content_type: Option<String>,
    /// Raw file bytes (already downloaded from platform CDN).
    pub data: Vec<u8>,
}

/// An attachment to send to an external platform.
#[derive(Debug, Clone)]
pub struct OutboundAttachment {
    /// Filename to present on the platform.
    pub filename: String,
    /// MIME content type.
    pub content_type: String,
    /// Raw file bytes.
    pub data: Vec<u8>,
}

/// Events emitted by platform bridges into the gateway event loop.
#[derive(Debug)]
pub enum GatewayEvent {
    /// A message was received from an external platform.
    MessageReceived {
        /// The platform that received the message.
        platform_id: PlatformId,
        /// The external channel ID the message arrived in.
        channel_id: String,
        /// The external user ID of the sender.
        user_id: String,
        /// The external username of the sender.
        username: String,
        /// The message text content.
        content: String,
        /// File attachments from the platform message.
        attachments: Vec<InboundAttachment>,
    },
    /// An external channel was deleted and its mapping should be removed.
    ChannelDeleted {
        /// The platform where the channel was deleted.
        platform_id: PlatformId,
        /// The deleted external channel ID.
        channel_id: String,
    },
}

/// A message to send to an external platform channel.
#[derive(Debug, Clone)]
pub struct PlatformMessage {
    /// The message text.
    pub text: String,
    /// The formatting mode.
    pub format: MessageFormat,
    /// Optional external message ID to reply to.
    pub reply_to: Option<String>,
    /// File attachments to send with the message.
    pub attachments: Vec<OutboundAttachment>,
}

/// Message formatting options for external platforms.
#[derive(Debug, Clone, Copy)]
pub enum MessageFormat {
    /// Markdown-formatted message.
    Markdown,
    /// Plain text message.
    Plain,
}

/// A channel visible to the bot on an external platform.
#[derive(Debug, Clone)]
pub struct ExternalChannel {
    /// The external channel ID.
    pub id: String,
    /// The human-readable channel name.
    pub name: String,
    /// The channel kind (e.g. "text", "dm", "thread").
    pub kind: String,
}

/// Configuration for connecting to an external platform.
#[derive(Debug, Clone)]
pub struct PlatformConfig {
    /// The platform ID in the database.
    pub platform_id: PlatformId,
    /// Platform-specific credentials.
    pub credentials: PlatformCredentials,
}

/// Platform-specific credentials for connecting to external services.
#[derive(Debug, Clone)]
pub enum PlatformCredentials {
    /// Discord bot credentials.
    Discord {
        /// Discord bot token.
        bot_token: String,
    },
    /// Telegram bot credentials.
    Telegram {
        /// Telegram bot token.
        bot_token: String,
    },
    /// Matrix homeserver credentials.
    Matrix {
        /// Matrix homeserver URL.
        homeserver_url: String,
        /// Matrix access token.
        access_token: String,
    },
    /// WhatsApp Business API credentials.
    Whatsapp {
        /// WhatsApp Business phone number ID.
        phone_number_id: String,
        /// WhatsApp Business API access token.
        access_token: String,
    },
}
