//! Skill-specific error types.

use sober_core::error::AppError;

/// Errors that can occur during skill operations.
#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    /// Skill not found in catalog.
    #[error("Skill not found: {0}")]
    NotFound(String),

    /// Failed to parse SKILL.md frontmatter.
    #[error("Failed to parse SKILL.md frontmatter in {path}: {reason}")]
    FrontmatterParseFailed { path: String, reason: String },

    /// Failed to read skill file or directory.
    #[error("Failed to read skill: {0}")]
    IoError(#[from] std::io::Error),

    /// Skill already activated in this conversation.
    #[error("Skill already active: {0}")]
    AlreadyActive(String),
}

impl From<SkillError> for AppError {
    fn from(err: SkillError) -> Self {
        match err {
            SkillError::NotFound(msg) => AppError::NotFound(msg),
            SkillError::AlreadyActive(msg) => AppError::Conflict(msg),
            SkillError::FrontmatterParseFailed { path, reason } => {
                AppError::Internal(Box::new(std::io::Error::other(format!(
                    "skill frontmatter parse failed in {path}: {reason}"
                ))))
            }
            SkillError::IoError(e) => AppError::Internal(Box::new(e)),
        }
    }
}
