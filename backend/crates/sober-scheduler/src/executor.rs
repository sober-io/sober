//! Job executor trait and registry for scheduler-local execution.
//!
//! Deterministic jobs (Artifact, Internal) run inside the scheduler process
//! instead of being dispatched to the agent. The [`JobExecutorRegistry`] maps
//! operation identifiers to concrete [`JobExecutor`] implementations, keeping
//! `TickEngine` free of domain-specific dependencies.

use std::collections::HashMap;
use std::sync::Arc;

use sober_core::error::AppError;
use sober_core::types::Job;

/// Result of a locally executed job.
pub struct ExecutionResult {
    /// Human-readable summary of what happened.
    pub summary: String,
    /// Optional reference to a produced artifact (e.g. blob key).
    pub artifact_ref: Option<String>,
}

/// A single job executor for a specific operation type.
///
/// Implementations are registered in [`JobExecutorRegistry`] and invoked by the
/// tick engine when a matching job becomes due.
#[tonic::async_trait]
pub trait JobExecutor: Send + Sync {
    /// Execute the job, returning a human-readable summary.
    async fn execute(&self, job: &Job) -> Result<ExecutionResult, AppError>;
}

/// Registry of local job executors keyed by operation identifier.
///
/// Built once at startup in `main.rs` and shared (via `Arc`) with the tick
/// engine. Operation keys match the `"op"` field in the job's JSON payload.
pub struct JobExecutorRegistry {
    executors: HashMap<String, Arc<dyn JobExecutor>>,
}

impl JobExecutorRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            executors: HashMap::new(),
        }
    }

    /// Register an executor for the given operation key.
    pub fn register(&mut self, op: &str, executor: Arc<dyn JobExecutor>) {
        self.executors.insert(op.to_owned(), executor);
    }

    /// Look up an executor by operation key.
    pub fn get(&self, op: &str) -> Option<&Arc<dyn JobExecutor>> {
        self.executors.get(op)
    }
}

impl Default for JobExecutorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubExecutor;

    #[tonic::async_trait]
    impl JobExecutor for StubExecutor {
        async fn execute(&self, _job: &Job) -> Result<ExecutionResult, AppError> {
            Ok(ExecutionResult {
                summary: "stub executed".into(),
                artifact_ref: None,
            })
        }
    }

    #[test]
    fn register_and_retrieve_executor() {
        let mut registry = JobExecutorRegistry::new();
        registry.register("test_op", Arc::new(StubExecutor));

        assert!(registry.get("test_op").is_some());
        assert!(registry.get("unknown").is_none());
    }
}
