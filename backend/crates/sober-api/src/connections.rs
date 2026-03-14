//! WebSocket connection registry for routing conversation events.
//!
//! [`ConnectionRegistry`] maps conversation IDs to active WebSocket senders.
//! The subscription task uses this to route `ConversationUpdate` events from
//! the agent to the correct WebSocket connections.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, mpsc};
use tracing::debug;

use crate::routes::ws::ServerWsMessage;

/// Registry of active WebSocket connections per conversation.
///
/// Thread-safe and shared across the subscription task and WebSocket handlers.
#[derive(Clone)]
pub struct ConnectionRegistry {
    inner: Arc<RwLock<HashMap<String, Vec<mpsc::Sender<ServerWsMessage>>>>>,
}

impl ConnectionRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Registers a sender for the given conversation.
    ///
    /// Multiple senders can be registered per conversation (e.g. the same
    /// conversation open in multiple browser tabs).
    pub async fn register(&self, conversation_id: String, sender: mpsc::Sender<ServerWsMessage>) {
        let mut map = self.inner.write().await;
        map.entry(conversation_id).or_default().push(sender);
    }

    /// Unregisters all senders associated with the given sender pointer.
    ///
    /// Called when a WebSocket disconnects. Removes senders that match by
    /// checking if they are closed (the corresponding receiver was dropped).
    pub async fn unregister(&self, conversation_id: &str) {
        let mut map = self.inner.write().await;
        if let Some(senders) = map.get_mut(conversation_id) {
            senders.retain(|s| !s.is_closed());
            if senders.is_empty() {
                map.remove(conversation_id);
            }
        }
    }

    /// Sends a message to all registered senders for a conversation.
    ///
    /// Automatically removes closed senders. Returns the number of senders
    /// that received the message.
    pub async fn send(&self, conversation_id: &str, msg: ServerWsMessage) -> usize {
        let mut map = self.inner.write().await;
        let Some(senders) = map.get_mut(conversation_id) else {
            return 0;
        };

        let mut delivered = 0;
        let mut to_remove = Vec::new();

        for (i, sender) in senders.iter().enumerate() {
            if sender.is_closed() {
                to_remove.push(i);
                continue;
            }
            match sender.try_send(msg.clone()) {
                Ok(()) => delivered += 1,
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    to_remove.push(i);
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    debug!(
                        conversation_id,
                        "WebSocket send buffer full, dropping event"
                    );
                }
            }
        }

        // Remove dead senders in reverse order to preserve indices.
        for i in to_remove.into_iter().rev() {
            senders.swap_remove(i);
        }
        if senders.is_empty() {
            map.remove(conversation_id);
        }

        delivered
    }
}

impl Default for ConnectionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_and_send() {
        let registry = ConnectionRegistry::new();
        let (tx, mut rx) = mpsc::channel(8);

        registry.register("conv-1".to_owned(), tx).await;

        let msg = ServerWsMessage::ChatDone {
            conversation_id: "conv-1".to_owned(),
            message_id: "msg-1".to_owned(),
        };
        let delivered = registry.send("conv-1", msg).await;
        assert_eq!(delivered, 1);

        let received = rx.recv().await.expect("should receive message");
        match received {
            ServerWsMessage::ChatDone { message_id, .. } => {
                assert_eq!(message_id, "msg-1");
            }
            _ => panic!("unexpected message type"),
        }
    }

    #[tokio::test]
    async fn send_to_unknown_conversation() {
        let registry = ConnectionRegistry::new();
        let msg = ServerWsMessage::Pong;
        let delivered = registry.send("nonexistent", msg).await;
        assert_eq!(delivered, 0);
    }

    #[tokio::test]
    async fn dead_senders_are_cleaned_up() {
        let registry = ConnectionRegistry::new();
        let (tx, rx) = mpsc::channel(8);

        registry.register("conv-1".to_owned(), tx).await;
        drop(rx); // Close the receiver.

        let msg = ServerWsMessage::Pong;
        let delivered = registry.send("conv-1", msg).await;
        assert_eq!(delivered, 0);
    }
}
