//! LLM error types.

use std::time::Duration;

use sober_core::error::AppError;

/// Errors produced by LLM operations.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    /// Provider returned a non-2xx HTTP status.
    #[error("API error (HTTP {status}): {message}")]
    ApiError {
        /// HTTP status code.
        status: u16,
        /// Error message from provider.
        message: String,
    },

    /// Provider returned 429 — too many requests.
    #[error("Rate limited")]
    RateLimited {
        /// Suggested wait time before retrying, if the provider included one.
        retry_after: Option<Duration>,
    },

    /// Network-level failure (DNS, TLS, connection refused, timeout, etc.).
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    /// Error while parsing an SSE stream or ACP notification stream.
    #[error("Stream error: {0}")]
    StreamError(String),

    /// The requested operation is not supported by this engine.
    #[error("Unsupported: {0}")]
    Unsupported(String),

    /// Response body could not be deserialized into the expected type.
    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    /// JSON-RPC protocol error (ACP transport).
    #[error("JSON-RPC error ({code}): {message}")]
    JsonRpcError {
        /// JSON-RPC error code.
        code: i64,
        /// Error message.
        message: String,
    },

    /// ACP subprocess exited unexpectedly or failed to start.
    #[error("ACP process error: {0}")]
    ProcessError(String),
}

impl From<LlmError> for AppError {
    fn from(err: LlmError) -> Self {
        AppError::Internal(Box::new(err))
    }
}
