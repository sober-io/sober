//! Vector storage, memory pruning, and scoped retrieval for the Sober system.
//!
//! This crate is the sole interface to Qdrant — no other crate should access
//! the vector database directly.

pub mod error;
pub mod loader;
pub mod scoring;
pub mod store;

pub use error::MemoryError;
pub use loader::{ContextLoader, LoadRequest, LoadedContext};
pub use scoring::{boost, decay, should_prune};
pub use store::{
    ChunkType, CollectionTarget, DedupStats, MemoryHit, MemoryStore, StoreChunk, StoreOutcome,
    StoreQuery,
};
