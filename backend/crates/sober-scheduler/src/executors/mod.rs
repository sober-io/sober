//! Concrete job executor implementations.
//!
//! Each executor handles a specific deterministic operation that the scheduler
//! can run locally without involving the agent's LLM pipeline.

pub mod artifact;
pub mod memory_pruning;
pub mod session_cleanup;
