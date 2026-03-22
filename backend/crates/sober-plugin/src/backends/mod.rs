//! Backend trait abstractions for plugin host function services.
//!
//! Each backend trait defines an object-safe interface that can be
//! implemented for both in-memory (tests/offline) and database-backed
//! (production) usage.  Host functions receive a single `Arc<dyn Backend>`
//! instead of branching on `Option<PgPool>` internally.

pub mod kv;

pub use kv::{InMemoryKvBackend, KvBackend, PgKvBackend};
