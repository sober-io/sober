//! Ed25519 keypair generation, signing, and verification.
//!
//! Wraps `ed25519-dalek` with a minimal API. Keys are 32 bytes each;
//! signatures are 64 bytes. All randomness comes from the OS CSPRNG.

use ed25519_dalek::{Signer, Verifier};
use rand_core::OsRng;

use crate::error::CryptoError;

// Re-export the key and signature types so callers don't need to depend
// on ed25519-dalek directly.
pub use ed25519_dalek::{Signature, SigningKey, VerifyingKey};

/// Generate a fresh Ed25519 keypair from the OS CSPRNG.
pub fn generate_keypair() -> (SigningKey, VerifyingKey) {
    let signing = SigningKey::generate(&mut OsRng);
    let verifying = signing.verifying_key();
    (signing, verifying)
}

/// Sign a message with an Ed25519 signing key.
///
/// The signature is deterministic: the same key and message always
/// produce the same signature.
pub fn sign(key: &SigningKey, message: &[u8]) -> Signature {
    key.sign(message)
}

/// Verify an Ed25519 signature against a verifying (public) key.
///
/// Returns `Ok(())` if the signature is valid, or a [`CryptoError`] if
/// verification fails.
pub fn verify(
    key: &VerifyingKey,
    message: &[u8],
    signature: &Signature,
) -> Result<(), CryptoError> {
    key.verify(message, signature)
        .map_err(|e| CryptoError::Signature(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_roundtrip() {
        let (signing, verifying) = generate_keypair();
        let message = b"hello sober";
        let sig = sign(&signing, message);
        assert!(verify(&verifying, message, &sig).is_ok());
    }

    #[test]
    fn wrong_key_rejects_signature() {
        let (signing, _) = generate_keypair();
        let (_, wrong_verifying) = generate_keypair();
        let message = b"hello sober";
        let sig = sign(&signing, message);
        assert!(verify(&wrong_verifying, message, &sig).is_err());
    }

    #[test]
    fn tampered_message_rejects() {
        let (signing, verifying) = generate_keypair();
        let sig = sign(&signing, b"original");
        assert!(verify(&verifying, b"tampered", &sig).is_err());
    }

    #[test]
    fn deterministic_signatures() {
        let (signing, _) = generate_keypair();
        let message = b"deterministic";
        let sig1 = sign(&signing, message);
        let sig2 = sign(&signing, message);
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn empty_message_signs_and_verifies() {
        let (signing, verifying) = generate_keypair();
        let sig = sign(&signing, b"");
        assert!(verify(&verifying, b"", &sig).is_ok());
    }

    #[test]
    fn key_bytes_roundtrip() {
        let (signing, verifying) = generate_keypair();

        let signing_bytes = signing.to_bytes();
        let verifying_bytes = verifying.to_bytes();

        let restored_signing = SigningKey::from_bytes(&signing_bytes);
        let restored_verifying =
            VerifyingKey::from_bytes(&verifying_bytes).expect("valid verifying key bytes");

        let message = b"roundtrip";
        let sig = sign(&restored_signing, message);
        assert!(verify(&restored_verifying, message, &sig).is_ok());
    }
}
