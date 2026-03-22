//! Memory read/write backend trait.
//!
//! [`MemoryBackend`] provides an object-safe interface for searching and
//! storing memory chunks on behalf of a plugin.  Implementations will
//! typically delegate to `sober-memory` for vector search and storage.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

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
