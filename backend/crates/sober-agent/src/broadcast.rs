//! Broadcast channel for conversation update events.
//!
//! The agent publishes all conversation events to a [`tokio::sync::broadcast`]
//! channel. Subscribers (e.g. the API via `SubscribeConversationUpdates` gRPC)
//! receive events for all conversations and route them by `conversation_id`.

use crate::grpc::proto;

/// Sender half of the conversation update broadcast channel.
pub type ConversationUpdateSender = tokio::sync::broadcast::Sender<proto::ConversationUpdate>;

/// Receiver half of the conversation update broadcast channel.
pub type ConversationUpdateReceiver = tokio::sync::broadcast::Receiver<proto::ConversationUpdate>;

/// Default capacity for the broadcast channel.
///
/// Events are dropped if all receivers lag behind by more than this many
/// messages. This is acceptable because the database is the source of truth.
const BROADCAST_CAPACITY: usize = 256;

/// Creates a new broadcast channel for conversation updates.
///
/// Returns the sender (shared with the agent) and a receiver (used to
/// create additional subscriptions via `sender.subscribe()`).
pub fn create_broadcast_channel() -> (ConversationUpdateSender, ConversationUpdateReceiver) {
    tokio::sync::broadcast::channel(BROADCAST_CAPACITY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_channel_creation() {
        let (tx, _rx) = create_broadcast_channel();
        // Should be able to subscribe additional receivers.
        let _rx2 = tx.subscribe();
    }
}
