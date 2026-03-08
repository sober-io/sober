//! Memory subsystem error types.

use sober_core::error::AppError;

/// Errors specific to the memory subsystem.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// Qdrant client or server error.
    #[error("qdrant error: {0}")]
    Qdrant(String),

    /// BCF format validation failure.
    #[error("bcf format error: {0}")]
    BcfFormat(String),

    /// Unknown chunk type discriminant.
    #[error("invalid chunk type: {0}")]
    InvalidChunkType(u8),

    /// Repository (database) error.
    #[error("repository error: {0}")]
    Repo(String),

    /// Scope could not be resolved.
    #[error("scope not found: {0}")]
    ScopeNotFound(String),

    /// Token budget exceeded during context loading.
    #[error("token budget exhausted: requested {requested}, budget {budget}")]
    BudgetExhausted {
        /// Tokens attempted to include.
        requested: usize,
        /// Maximum allowed tokens.
        budget: usize,
    },
}

impl From<MemoryError> for AppError {
    fn from(err: MemoryError) -> Self {
        AppError::Internal(Box::new(err))
    }
}
