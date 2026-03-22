//! Conversation reading backend trait.
//!
//! [`ConversationBackend`] provides an object-safe interface for reading
//! conversation history.  Implementations will typically delegate to the
//! message repository in `sober-db`.

use std::future::Future;
use std::pin::Pin;

use serde::Serialize;

/// Object-safe backend for reading conversation messages.
///
/// Allows plugins to access recent messages from a conversation,
/// enabling context-aware processing.
pub trait ConversationBackend: Send + Sync {
    /// Lists recent messages from a conversation.
    fn list_messages(
        &self,
        conversation_id: &str,
        limit: Option<u32>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ConversationMessage>, String>> + Send + '_>>;
}

/// A message from a conversation.
#[derive(Debug, Clone, Serialize)]
pub struct ConversationMessage {
    /// The role of the message author (e.g. "user", "assistant", "system").
    pub role: String,
    /// The textual content of the message.
    pub content: String,
    /// ISO 8601 timestamp of when the message was created.
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Compile-time assertions
// ---------------------------------------------------------------------------

// ConversationBackend is object-safe and dyn-compatible.
#[allow(dead_code)]
const _: () = {
    fn assert_object_safe(_: &dyn ConversationBackend) {}
};

// Arc<dyn ConversationBackend> is Send + Sync.
#[allow(dead_code)]
const _: () = {
    fn assert_send_sync<T: Send + Sync>() {}
    fn check() {
        assert_send_sync::<std::sync::Arc<dyn ConversationBackend>>();
    }
};
