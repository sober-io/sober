//! Agent-specific error types.

use sober_core::AppError;

/// Errors that can occur during agent operations.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// A tool execution failed.
    #[error("tool execution failed: {0}")]
    ToolExecutionFailed(String),

    /// The agent exceeded the maximum number of tool iterations.
    #[error("max tool iterations exceeded ({0})")]
    MaxIterationsExceeded(u32),

    /// Failed to load context for the agent.
    #[error("context load failed: {0}")]
    ContextLoadFailed(String),

    /// An LLM API call failed.
    #[error("LLM call failed: {0}")]
    LlmCallFailed(String),

    /// Prompt injection was detected in user input.
    #[error("injection detected: {0}")]
    InjectionDetected(String),

    /// An internal error that doesn't fit other categories.
    #[error("{0}")]
    Internal(String),
}

impl From<AgentError> for AppError {
    fn from(e: AgentError) -> Self {
        match e {
            AgentError::InjectionDetected(reason) => {
                AppError::Validation(format!("injection detected: {reason}"))
            }
            other => AppError::Internal(other.into()),
        }
    }
}
