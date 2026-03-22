//! Memory read/write backend trait and implementations.
//!
//! [`MemoryBackend`] provides an object-safe interface for searching and
//! storing memory chunks on behalf of a plugin.  Implementations will
//! typically delegate to `sober-memory` for vector search and storage.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::Serialize;
use sober_core::types::ids::UserId;

/// Object-safe backend for memory search and storage.
///
/// Memory is scoped to a user.  Plugins can search for relevant knowledge
/// and store new facts/observations back into the memory system.
pub trait MemoryBackend: Send + Sync {
    /// Searches memory for the given query.
    fn search(
        &self,
        user_id: UserId,
        query: &str,
        scope: Option<&str>,
        limit: Option<u32>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemoryHit>, String>> + Send + '_>>;

    /// Stores a memory chunk.
    fn store(
        &self,
        user_id: UserId,
        content: &str,
        scope: Option<&str>,
        metadata: HashMap<String, serde_json::Value>,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;
}

/// A single memory search result.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryHit {
    /// The textual content of the memory chunk.
    pub content: String,
    /// Similarity score (0.0 – 1.0, higher is more relevant).
    pub score: f64,
    /// Optional chunk type (e.g. "fact", "conversation", "skill").
    pub chunk_type: Option<String>,
}

// ---------------------------------------------------------------------------
// QdrantMemoryBackend
// ---------------------------------------------------------------------------

/// Qdrant-backed memory backend for production use.
///
/// Delegates to [`sober_memory::MemoryStore`] for vector search and storage.
/// Uses the LLM engine to embed query text and store content into dense
/// vectors before passing them to Qdrant.
pub struct QdrantMemoryBackend {
    memory: Arc<sober_memory::MemoryStore>,
    llm: Arc<dyn sober_llm::LlmEngine>,
}

impl QdrantMemoryBackend {
    /// Creates a new Qdrant-backed memory backend.
    pub fn new(memory: Arc<sober_memory::MemoryStore>, llm: Arc<dyn sober_llm::LlmEngine>) -> Self {
        Self { memory, llm }
    }
}

impl MemoryBackend for QdrantMemoryBackend {
    fn search(
        &self,
        user_id: UserId,
        query: &str,
        _scope: Option<&str>,
        limit: Option<u32>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemoryHit>, String>> + Send + '_>> {
        let memory = Arc::clone(&self.memory);
        let llm = Arc::clone(&self.llm);
        let query_text = query.to_owned();
        let limit = u64::from(limit.unwrap_or(10));
        Box::pin(async move {
            // Embed the query text
            let embeddings = llm
                .embed(&[query_text.as_str()])
                .await
                .map_err(|e| format!("embedding failed: {e}"))?;

            let dense_vector = embeddings
                .into_iter()
                .next()
                .ok_or_else(|| "no embedding returned".to_owned())?;

            let store_query = sober_memory::StoreQuery {
                dense_vector,
                query_text,
                scope_id: sober_core::ScopeId::from_uuid(uuid::Uuid::from_bytes(
                    user_id.as_uuid().into_bytes(),
                )),
                limit,
                score_threshold: None,
                chunk_type_filter: None,
            };

            let hits = memory
                .search(user_id, store_query)
                .await
                .map_err(|e| format!("memory search failed: {e}"))?;

            Ok(hits
                .into_iter()
                .map(|h| MemoryHit {
                    content: h.content,
                    score: f64::from(h.score),
                    chunk_type: Some(h.chunk_type.to_string()),
                })
                .collect())
        })
    }

    fn store(
        &self,
        user_id: UserId,
        content: &str,
        _scope: Option<&str>,
        _metadata: HashMap<String, serde_json::Value>,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
        let memory = Arc::clone(&self.memory);
        let llm = Arc::clone(&self.llm);
        let content = content.to_owned();
        Box::pin(async move {
            // Embed the content
            let embeddings = llm
                .embed(&[content.as_str()])
                .await
                .map_err(|e| format!("embedding failed: {e}"))?;

            let dense_vector = embeddings
                .into_iter()
                .next()
                .ok_or_else(|| "no embedding returned".to_owned())?;

            let chunk = sober_memory::StoreChunk {
                dense_vector,
                content,
                chunk_type: sober_memory::ChunkType::Fact,
                scope_id: sober_core::ScopeId::from_uuid(uuid::Uuid::from_bytes(
                    user_id.as_uuid().into_bytes(),
                )),
                source_message_id: None,
                importance: 0.5,
                decay_at: chrono::Utc::now() + chrono::Duration::days(30),
            };

            let point_id = memory
                .store(user_id, chunk)
                .await
                .map_err(|e| format!("memory store failed: {e}"))?;

            Ok(point_id.to_string())
        })
    }
}

// ---------------------------------------------------------------------------
// Compile-time assertions
// ---------------------------------------------------------------------------

// MemoryBackend is object-safe and dyn-compatible.
#[allow(dead_code)]
const _: () = {
    fn assert_object_safe(_: &dyn MemoryBackend) {}
};

// Arc<dyn MemoryBackend> is Send + Sync.
#[allow(dead_code)]
const _: () = {
    fn assert_send_sync<T: Send + Sync>() {}
    fn check() {
        assert_send_sync::<std::sync::Arc<dyn MemoryBackend>>();
    }
};
