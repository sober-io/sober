//! Types for the Qdrant memory store.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sober_core::{ConversationId, MessageId, ScopeId, UserId};

/// Memory chunk type discriminant.
///
/// Categorises knowledge extracted from conversations before storage
/// in the vector database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum ChunkType {
    /// Extracted knowledge fact.
    Fact = 0,
    /// User preference or personal setting.
    Preference = 1,
    /// Decision or choice made, with rationale.
    Decision = 2,
    /// Soul layer data (internal, used by sober-mind).
    Soul = 3,
}

impl std::fmt::Display for ChunkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Fact => "fact",
            Self::Preference => "preference",
            Self::Decision => "decision",
            Self::Soul => "soul",
        };
        f.write_str(s)
    }
}

impl From<ChunkType> for u8 {
    fn from(ct: ChunkType) -> Self {
        ct as u8
    }
}

impl TryFrom<u8> for ChunkType {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Fact),
            1 => Ok(Self::Preference),
            2 => Ok(Self::Decision),
            3 => Ok(Self::Soul),
            other => Err(other),
        }
    }
}

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
    /// Optional chunk type filter (as u8 discriminant).
    pub chunk_type_filter: Option<u8>,
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
    /// When this memory starts decaying.
    pub decay_at: DateTime<Utc>,
}

/// Outcome of a dedup-aware store operation.
#[derive(Debug)]
pub enum StoreOutcome {
    /// A new point was stored (no duplicate found).
    Stored { point_id: uuid::Uuid },
    /// A duplicate was found; the existing point was boosted instead.
    Deduplicated {
        existing_point_id: uuid::Uuid,
        similarity: f32,
    },
}

/// Target collection for batch operations.
#[derive(Debug, Clone)]
pub enum CollectionTarget {
    /// A specific user's collection.
    User(UserId),
    /// A specific conversation's collection.
    Conversation(ConversationId),
    /// The system-wide collection.
    System,
}

/// Summary statistics from a batch deduplication run.
#[derive(Debug, Default)]
pub struct DedupStats {
    /// Number of points scanned.
    pub scanned: u64,
    /// Number of duplicate points deleted.
    pub merged: u64,
}
