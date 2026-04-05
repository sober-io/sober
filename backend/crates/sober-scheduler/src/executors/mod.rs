//! Concrete job executor implementations.
//!
//! Each executor handles a specific deterministic operation that the scheduler
//! can run locally without involving the agent's LLM pipeline.

pub mod artifact;
pub mod attachment_cleanup;
pub mod blob_gc;
pub mod memory_dedup;
pub mod memory_orphan_cleanup;
pub mod memory_pruning;
pub mod plugin_cleanup;
pub mod session_cleanup;
