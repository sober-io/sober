//! Vector storage, Binary Context Format (BCF), memory pruning, and scoped
//! retrieval for the Sober system.
//!
//! This crate is the sole interface to Qdrant — no other crate should access
//! the vector database directly.

pub mod bcf;
pub mod error;
pub mod loader;
pub mod scoring;
pub mod store;

pub use bcf::{BcfChunk, BcfHeader, BcfReader, BcfWriter, ChunkType};
pub use error::MemoryError;
pub use loader::{ContextLoader, LoadRequest, LoadedContext};
pub use scoring::{boost, decay, should_prune};
pub use store::{MemoryHit, MemoryStore, StoreChunk, StoreQuery};
