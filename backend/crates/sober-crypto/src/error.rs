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

    /// Input data is malformed (wrong length, bad encoding, etc.).
    #[error("invalid data: {0}")]
    InvalidData(String),

    /// Symmetric encryption failure (AES-256-GCM).
    #[error("encryption error: {0}")]
    EncryptionError(String),

    /// Symmetric decryption failure (AES-256-GCM — wrong key, tampered data).
    #[error("decryption error: {0}")]
    DecryptionError(String),
}

impl From<CryptoError> for AppError {
    fn from(err: CryptoError) -> Self {
        AppError::Internal(Box::new(err))
    }
}
