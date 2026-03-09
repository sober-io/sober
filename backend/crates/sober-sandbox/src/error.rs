//! Sandbox error types.

use sober_core::error::AppError;

/// Errors specific to the sandbox subsystem.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// Bubblewrap binary not found on PATH.
    #[error("bwrap not found: install bubblewrap (e.g. `apt install bubblewrap`)")]
    BwrapNotFound,

    /// Socat binary not found on PATH (required for AllowedDomains network mode).
    #[error("socat not found: install socat (e.g. `apt install socat`)")]
    SocatNotFound,

    /// Bwrap process failed to start.
    #[error("sandbox spawn failed: {0}")]
    SpawnFailed(String),

    /// Execution exceeded the configured timeout.
    #[error("sandbox execution timed out after {seconds}s")]
    Timeout {
        /// The configured timeout in seconds.
        seconds: u32,
    },

    /// Proxy setup or operation failed.
    #[error("proxy error: {0}")]
    ProxyFailed(String),

    /// Policy resolution or config parsing error.
    #[error("policy resolution failed: {0}")]
    PolicyResolutionFailed(String),

    /// Process was killed (SIGKILL after grace period).
    #[error("sandboxed process was killed")]
    Killed,
}

impl From<SandboxError> for AppError {
    fn from(err: SandboxError) -> Self {
        AppError::Internal(Box::new(err))
    }
}
