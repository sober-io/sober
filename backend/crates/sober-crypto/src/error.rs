//! Cryptographic error types.

use sober_core::error::AppError;

/// Errors produced by cryptographic operations.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    /// Password hashing failure (e.g. invalid parameters).
    #[error("hash error: {0}")]
    Hash(String),

    /// Password or signature verification failed.
    #[error("verification error: {0}")]
    Verification(String),

    /// Keypair generation failure.
    #[error("key generation error: {0}")]
    KeyGeneration(String),

    /// Signing or signature validation failure.
    #[error("signature error: {0}")]
    Signature(String),
}

impl From<CryptoError> for AppError {
    fn from(err: CryptoError) -> Self {
        AppError::Internal(Box::new(err))
    }
}
