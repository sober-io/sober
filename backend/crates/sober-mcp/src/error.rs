//! MCP-specific error types.

use sober_core::error::AppError;

/// Errors that can occur during MCP client operations.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    /// Failed to connect to or start the MCP server process.
    #[error("MCP connection failed: {0}")]
    ConnectionFailed(String),

    /// The MCP initialize handshake failed.
    #[error("MCP initialize failed: {0}")]
    InitializeFailed(String),

    /// A tool call to the MCP server failed.
    #[error("MCP tool call failed: {0}")]
    ToolCallFailed(String),

    /// A resource read from the MCP server failed.
    #[error("MCP resource read failed: {0}")]
    ResourceReadFailed(String),

    /// Protocol-level error (malformed JSON-RPC, unexpected response).
    #[error("MCP protocol error: {0}")]
    ProtocolError(String),

    /// The MCP server is not available (crashed or not started).
    #[error("MCP server unavailable: {0}")]
    ServerUnavailable(String),

    /// Request to the MCP server timed out.
    #[error("MCP request timed out after {seconds}s")]
    Timeout {
        /// The timeout duration in seconds.
        seconds: u64,
    },

    /// Sandbox-related error when spawning the MCP server process.
    #[error("MCP sandbox error: {0}")]
    Sandbox(#[from] sober_sandbox::SandboxError),
}

impl From<McpError> for AppError {
    fn from(err: McpError) -> Self {
        AppError::Internal(Box::new(err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_failed_display() {
        let err = McpError::ConnectionFailed("process exited".into());
        assert_eq!(err.to_string(), "MCP connection failed: process exited");
    }

    #[test]
    fn initialize_failed_display() {
        let err = McpError::InitializeFailed("version mismatch".into());
        assert_eq!(err.to_string(), "MCP initialize failed: version mismatch");
    }

    #[test]
    fn tool_call_failed_display() {
        let err = McpError::ToolCallFailed("invalid arguments".into());
        assert_eq!(err.to_string(), "MCP tool call failed: invalid arguments");
    }

    #[test]
    fn resource_read_failed_display() {
        let err = McpError::ResourceReadFailed("not found".into());
        assert_eq!(err.to_string(), "MCP resource read failed: not found");
    }

    #[test]
    fn protocol_error_display() {
        let err = McpError::ProtocolError("invalid JSON".into());
        assert_eq!(err.to_string(), "MCP protocol error: invalid JSON");
    }

    #[test]
    fn server_unavailable_display() {
        let err = McpError::ServerUnavailable("crashed".into());
        assert_eq!(err.to_string(), "MCP server unavailable: crashed");
    }

    #[test]
    fn timeout_display() {
        let err = McpError::Timeout { seconds: 30 };
        assert_eq!(err.to_string(), "MCP request timed out after 30s");
    }

    #[test]
    fn converts_to_app_error() {
        let err = McpError::ConnectionFailed("test".into());
        let app_err: AppError = err.into();
        assert!(matches!(app_err, AppError::Internal(_)));
    }
}
