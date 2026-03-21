//! Plugin system error types.

use sober_core::error::AppError;

/// Errors produced by the plugin system.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// The requested plugin was not found.
    #[error("plugin not found: {0}")]
    NotFound(String),

    /// The audit pipeline rejected the plugin.
    #[error("audit rejected at {stage}: {reason}")]
    AuditRejected {
        /// Which audit stage failed.
        stage: String,
        /// Why it was rejected.
        reason: String,
    },

    /// A required capability was not granted.
    #[error("capability denied: {0}")]
    CapabilityDenied(String),

    /// Plugin execution failed at runtime.
    #[error("plugin execution failed: {0}")]
    ExecutionFailed(String),

    /// Plugin compilation (e.g. WASM) failed.
    #[error("compilation failed: {0}")]
    CompilationFailed(String),

    /// The plugin manifest is invalid.
    #[error("manifest invalid: {0}")]
    ManifestInvalid(String),

    /// A plugin with the same name already exists.
    #[error("plugin already exists: {0}")]
    AlreadyExists(String),

    /// Plugin configuration is invalid or missing required fields.
    #[error("plugin config error: {0}")]
    Config(String),

    /// An MCP subsystem error.
    #[error("MCP error: {0}")]
    Mcp(#[from] sober_mcp::McpError),

    /// A skill subsystem error.
    #[error("skill error: {0}")]
    Skill(#[from] sober_skill::SkillError),

    /// An opaque internal error.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<PluginError> for AppError {
    fn from(e: PluginError) -> Self {
        match e {
            PluginError::NotFound(msg) => AppError::NotFound(msg),
            PluginError::ManifestInvalid(msg) => AppError::Validation(msg),
            PluginError::AlreadyExists(msg) => AppError::Conflict(msg),
            PluginError::CapabilityDenied(_) => AppError::Forbidden,
            PluginError::AuditRejected { stage, reason } => {
                AppError::Validation(format!("audit rejected at {stage}: {reason}"))
            }
            PluginError::Config(msg) => AppError::Validation(msg),
            PluginError::ExecutionFailed(msg) => AppError::Internal(msg.into()),
            PluginError::CompilationFailed(msg) => AppError::Internal(msg.into()),
            PluginError::Mcp(e) => AppError::Internal(Box::new(e)),
            PluginError::Skill(e) => AppError::Internal(Box::new(e)),
            PluginError::Internal(e) => AppError::Internal(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let e = PluginError::NotFound("test-plugin".into());
        assert_eq!(e.to_string(), "plugin not found: test-plugin");

        let e = PluginError::ManifestInvalid("missing name".into());
        assert_eq!(e.to_string(), "manifest invalid: missing name");
    }

    #[test]
    fn into_app_error() {
        let e = PluginError::NotFound("x".into());
        let app: sober_core::error::AppError = e.into();
        assert!(matches!(app, sober_core::error::AppError::NotFound(_)));
    }
}
