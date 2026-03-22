//! Backend trait abstractions for plugin host function services.
//!
//! Each backend trait defines an object-safe interface that can be
//! implemented for both in-memory (tests/offline) and database-backed
//! (production) usage.  Host functions receive a single `Arc<dyn Backend>`
//! instead of branching on `Option<PgPool>` internally.

pub mod conversation;
pub mod kv;
pub mod memory;
pub mod schedule;
pub mod secrets;
pub mod tool_call;

pub use conversation::{ConversationBackend, ConversationMessage, PgConversationBackend};
pub use kv::{InMemoryKvBackend, KvBackend, PgKvBackend};
pub use memory::{MemoryBackend, MemoryHit, QdrantMemoryBackend};
pub use schedule::ScheduleBackend;
pub use secrets::{PgSecretBackend, SecretBackend};
pub use tool_call::ToolExecutor;
