//! Conversation reading backend trait and implementations.
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
// PgConversationBackend
// ---------------------------------------------------------------------------

/// PostgreSQL-backed conversation backend for production use.
///
/// Queries the `messages` table directly for a given conversation ID.
#[derive(Debug, Clone)]
pub struct PgConversationBackend {
    pool: sqlx::PgPool,
}

impl PgConversationBackend {
    /// Creates a new PostgreSQL-backed conversation backend.
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

impl ConversationBackend for PgConversationBackend {
    fn list_messages(
        &self,
        conversation_id: &str,
        limit: Option<u32>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ConversationMessage>, String>> + Send + '_>> {
        let pool = self.pool.clone();
        let conversation_id = conversation_id.to_owned();
        let limit = i64::from(limit.unwrap_or(50));
        Box::pin(async move {
            let conv_uuid: uuid::Uuid = conversation_id
                .parse()
                .map_err(|e| format!("invalid conversation ID: {e}"))?;

            let rows: Vec<(String, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
                "SELECT role::text, content, created_at \
                 FROM messages \
                 WHERE conversation_id = $1 \
                 ORDER BY created_at ASC \
                 LIMIT $2",
            )
            .bind(conv_uuid)
            .bind(limit)
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("failed to list messages: {e}"))?;

            Ok(rows
                .into_iter()
                .map(|(role, content, created_at)| ConversationMessage {
                    role,
                    content,
                    created_at: created_at.to_rfc3339(),
                })
                .collect())
        })
    }
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
