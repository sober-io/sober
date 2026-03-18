//! Error types for the mind crate.

use sober_core::error::AppError;

/// Errors originating from agent identity and prompt assembly operations.
#[derive(Debug, thiserror::Error)]
pub enum MindError {
    /// SOUL.md file not found or unreadable.
    #[error("Failed to load SOUL.md: {0}")]
    SoulLoadFailed(String),

    /// Workspace SOUL.md layer violates base/user constraints.
    #[error("Failed to merge SOUL.md layers: {0}")]
    SoulMergeFailed(String),

    /// User input classified as a prompt injection attempt.
    #[error("Input rejected: {reason}")]
    InjectionRejected {
        /// Human-readable explanation of why the input was rejected.
        reason: String,
    },

    /// Prompt assembly could not complete.
    #[error("Prompt assembly failed: {0}")]
    AssemblyFailed(String),

    /// BCF soul layer storage operation failed.
    #[error("Soul layer store failed: {0}")]
    LayerStoreFailed(String),

    /// An instruction file could not be loaded from disk.
    #[error("Failed to load instruction file: {0}")]
    InstructionLoadFailed(String),

    /// YAML frontmatter in an instruction file could not be parsed.
    #[error("Failed to parse instruction frontmatter: {0}")]
    FrontmatterParseFailed(String),

    /// An `@path` reference in an instruction file could not be resolved.
    #[error("Failed to resolve @path reference: {0}")]
    ReferenceResolutionFailed(String),
}

impl From<MindError> for AppError {
    fn from(err: MindError) -> Self {
        match err {
            MindError::InjectionRejected { .. } => AppError::Forbidden,
            MindError::SoulLoadFailed(msg) => {
                AppError::Internal(Box::new(std::io::Error::other(msg)))
            }
            MindError::SoulMergeFailed(msg) => AppError::Validation(msg),
            MindError::AssemblyFailed(msg) => {
                AppError::Internal(Box::new(std::io::Error::other(msg)))
            }
            MindError::LayerStoreFailed(msg) => {
                AppError::Internal(Box::new(std::io::Error::other(msg)))
            }
            MindError::InstructionLoadFailed(msg) => {
                AppError::Internal(Box::new(std::io::Error::other(msg)))
            }
            MindError::FrontmatterParseFailed(msg) => AppError::Validation(msg),
            MindError::ReferenceResolutionFailed(msg) => AppError::Validation(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injection_rejected_maps_to_forbidden() {
        let err = MindError::InjectionRejected {
            reason: "instruction override".into(),
        };
        let app_err: AppError = err.into();
        assert!(matches!(app_err, AppError::Forbidden));
    }

    #[test]
    fn soul_load_failed_maps_to_internal() {
        let err = MindError::SoulLoadFailed("not found".into());
        let app_err: AppError = err.into();
        assert!(matches!(app_err, AppError::Internal(_)));
    }

    #[test]
    fn soul_merge_failed_maps_to_validation() {
        let err = MindError::SoulMergeFailed("constraint violation".into());
        let app_err: AppError = err.into();
        assert!(matches!(app_err, AppError::Validation(_)));
    }

    #[test]
    fn display_formats_correctly() {
        let err = MindError::InjectionRejected {
            reason: "role-play injection".into(),
        };
        assert_eq!(err.to_string(), "Input rejected: role-play injection");

        let err = MindError::SoulLoadFailed("file missing".into());
        assert_eq!(err.to_string(), "Failed to load SOUL.md: file missing");
    }
}
