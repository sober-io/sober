//! Common tool trait and types shared across tool implementations.
//!
//! All tools (MCP, plugin, built-in) implement the [`Tool`] trait. This
//! provides a unified interface for the agent to discover and execute tools.

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

/// A boxed, `Send`-able future — used as the return type of [`Tool::execute`]
/// so the trait remains dyn-compatible.
pub type BoxToolFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ToolOutput, ToolError>> + Send + 'a>>;

/// Metadata describing a tool's capabilities and input schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetadata {
    /// Tool name (unique within a provider).
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema describing the accepted input format.
    pub input_schema: serde_json::Value,
    /// If `true`, executing this tool may invalidate loaded context
    /// (e.g. memory writes, file modifications).
    pub context_modifying: bool,
}

/// Output returned by a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// The tool's output content (text, JSON, etc.).
    pub content: String,
    /// Whether this output represents an error condition.
    pub is_error: bool,
}

/// Errors that can occur during tool execution.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// The requested tool was not found.
    #[error("Tool not found: {0}")]
    NotFound(String),

    /// The input provided to the tool was invalid.
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// The tool execution failed.
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    /// The tool requires user confirmation before proceeding.
    ///
    /// Returned by tools (e.g. shell) when a command is classified as
    /// dangerous. The agent loop should handle the confirmation flow and
    /// re-execute with `"confirmed": true` in the input if approved.
    #[error("Needs confirmation: {reason}")]
    NeedsConfirmation {
        /// Unique ID for this confirmation request.
        confirm_id: String,
        /// The command or action needing approval.
        command: String,
        /// Risk classification (e.g. "Dangerous").
        risk_level: String,
        /// Human-readable explanation.
        reason: String,
    },

    /// An unexpected internal error occurred.
    #[error("{0}")]
    Internal(Box<dyn std::error::Error + Send + Sync>),
}

/// Common interface for all tool implementations.
///
/// Tools are discovered via [`metadata`](Tool::metadata) and invoked via
/// [`execute`](Tool::execute). The agent uses this trait to interact with
/// MCP servers, plugins, and built-in tools uniformly.
pub trait Tool: Send + Sync {
    /// Returns metadata describing this tool.
    fn metadata(&self) -> ToolMetadata;

    /// Executes the tool with the given JSON input.
    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_error_display() {
        let e = ToolError::NotFound("web_search".into());
        assert_eq!(e.to_string(), "Tool not found: web_search");

        let e = ToolError::InvalidInput("missing field 'query'".into());
        assert_eq!(e.to_string(), "Invalid input: missing field 'query'");

        let e = ToolError::ExecutionFailed("timeout".into());
        assert_eq!(e.to_string(), "Execution failed: timeout");

        let e = ToolError::Internal("oops".into());
        assert_eq!(e.to_string(), "oops");
    }
}
