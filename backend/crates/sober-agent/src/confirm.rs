//! Confirmation channel for interactive shell command approval.

use std::collections::HashMap;

use tokio::sync::{mpsc, oneshot};

/// A response from the user to a confirmation request.
#[derive(Debug)]
pub struct ConfirmResponse {
    /// The confirmation ID this response is for.
    pub confirm_id: String,
    /// Whether the user approved the command.
    pub approved: bool,
}

/// Handle for sending confirmation responses back to the agent.
#[derive(Debug, Clone)]
pub struct ConfirmationSender {
    tx: mpsc::Sender<ConfirmResponse>,
}

/// Broker that matches confirmation responses to pending requests.
pub struct ConfirmationBroker {
    pending: HashMap<String, oneshot::Sender<bool>>,
    rx: mpsc::Receiver<ConfirmResponse>,
}

impl ConfirmationBroker {
    /// Create a new broker and its sender handle.
    pub fn new() -> (Self, ConfirmationSender) {
        let (tx, rx) = mpsc::channel(32);
        let broker = Self {
            pending: HashMap::new(),
            rx,
        };
        let sender = ConfirmationSender { tx };
        (broker, sender)
    }

    /// Register a pending confirmation. Returns a oneshot receiver that
    /// resolves when the user responds.
    pub fn register(&mut self, confirm_id: String) -> oneshot::Receiver<bool> {
        let (tx, rx) = oneshot::channel();
        self.pending.insert(confirm_id, tx);
        rx
    }

    /// Process one incoming response. Call this in a select loop.
    pub async fn process_next(&mut self) -> Option<()> {
        let resp = self.rx.recv().await?;
        if let Some(tx) = self.pending.remove(&resp.confirm_id) {
            let _ = tx.send(resp.approved);
        }
        Some(())
    }
}

impl ConfirmationSender {
    /// Send a confirmation response.
    pub async fn respond(
        &self,
        confirm_id: String,
        approved: bool,
    ) -> Result<(), mpsc::error::SendError<ConfirmResponse>> {
        self.tx
            .send(ConfirmResponse {
                confirm_id,
                approved,
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn confirmation_roundtrip() {
        let (mut broker, sender) = ConfirmationBroker::new();
        let rx = broker.register("test-1".to_string());

        tokio::spawn(async move {
            sender.respond("test-1".to_string(), true).await.unwrap();
        });

        broker.process_next().await.unwrap();
        let approved = rx.await.unwrap();
        assert!(approved);
    }

    #[tokio::test]
    async fn confirmation_deny() {
        let (mut broker, sender) = ConfirmationBroker::new();
        let rx = broker.register("test-2".to_string());

        tokio::spawn(async move {
            sender.respond("test-2".to_string(), false).await.unwrap();
        });

        broker.process_next().await.unwrap();
        let approved = rx.await.unwrap();
        assert!(!approved);
    }

    #[tokio::test]
    async fn unknown_confirm_id_ignored() {
        let (mut broker, sender) = ConfirmationBroker::new();
        let _rx = broker.register("known".to_string());

        tokio::spawn(async move {
            sender.respond("unknown".to_string(), true).await.unwrap();
        });

        broker.process_next().await.unwrap();
        // known request is still pending (not resolved by unknown response)
    }
}
