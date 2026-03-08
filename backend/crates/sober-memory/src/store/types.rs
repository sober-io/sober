//! Types for the Qdrant memory store.

use chrono::{DateTime, Utc};
use sober_core::{MessageId, ScopeId};

use crate::bcf::ChunkType;

/// A memory chunk to be stored in Qdrant.
#[derive(Debug)]
pub struct StoreChunk {
    /// Pre-computed dense embedding vector.
    pub dense_vector: Vec<f32>,
    /// Text content (also indexed for BM25 sparse search).
    pub content: String,
    /// Type of memory chunk.
    pub chunk_type: ChunkType,
    /// Scope this chunk belongs to.
    pub scope_id: ScopeId,
    /// Optional link to the originating message.
    pub source_message_id: Option<MessageId>,
    /// Initial importance score (0.0..=1.0).
    pub importance: f64,
    /// When this memory should start decaying.
    pub decay_at: DateTime<Utc>,
}

/// Parameters for a hybrid search query.
#[derive(Debug)]
pub struct StoreQuery {
    /// Pre-computed dense query vector.
    pub dense_vector: Vec<f32>,
    /// Raw query text (tokenized internally for BM25).
    pub query_text: String,
    /// Scope to filter results to.
    pub scope_id: ScopeId,
    /// Maximum number of results to return.
    pub limit: u64,
    /// Minimum score threshold (optional).
    pub score_threshold: Option<f32>,
}

/// A single result from a hybrid search.
#[derive(Debug, Clone)]
pub struct MemoryHit {
    /// Qdrant point UUID.
    pub point_id: uuid::Uuid,
    /// Text content of the memory.
    pub content: String,
    /// Type of memory chunk.
    pub chunk_type: ChunkType,
    /// Scope this chunk belongs to.
    pub scope_id: ScopeId,
    /// Link to the originating message (if any).
    pub source_message_id: Option<MessageId>,
    /// Current importance score.
    pub importance: f64,
    /// Combined search score.
    pub score: f32,
    /// When the memory was created.
    pub created_at: DateTime<Utc>,
}
