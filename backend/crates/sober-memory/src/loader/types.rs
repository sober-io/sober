//! Types for the context loader.

use sober_core::{ConversationId, Message, UserId};

use crate::store::MemoryHit;

/// Parameters for loading context.
pub struct LoadRequest {
    /// Pre-computed dense query vector.
    pub query_vector: Vec<f32>,
    /// Raw query text for BM25 sparse matching.
    pub query_text: String,
    /// The user whose memory to search.
    pub user_id: UserId,
    /// The active conversation (for recent message retrieval).
    pub conversation_id: ConversationId,
    /// Maximum token budget for the entire loaded context.
    pub token_budget: usize,
    /// How many recent messages to include.
    pub recent_message_count: i64,
    /// Maximum memory hits to retrieve per scope.
    pub hits_per_scope: u64,
}

/// The assembled context returned by [`ContextLoader::load`].
#[derive(Debug)]
pub struct LoadedContext {
    /// Recent messages from PostgreSQL, oldest-first.
    pub recent_messages: Vec<Message>,
    /// Vector search hits from conversation scope (current conversation only).
    pub conversation_memories: Vec<MemoryHit>,
    /// Vector search hits from user scope.
    pub user_memories: Vec<MemoryHit>,
    /// Vector search hits from system scope.
    pub system_memories: Vec<MemoryHit>,
    /// Approximate total token count across all included content.
    pub estimated_tokens: usize,
}
