//! Context loader — combines Qdrant search with recent messages.

mod context_loader;
mod types;

pub use context_loader::ContextLoader;
pub use types::{LoadRequest, LoadedContext};
