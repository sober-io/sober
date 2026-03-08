//! Session token generation.
//!
//! Generates cryptographically random 256-bit session tokens and their
//! SHA-256 hashes. The raw token is sent to the client in a cookie;
//! only the hash is stored in the database.

use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

/// Generates a 256-bit (32-byte) cryptographically random session token.
///
/// Returns `(raw_hex, sha256_hex)` where:
/// - `raw_hex` is the 64-character hex-encoded token sent to the client.
/// - `sha256_hex` is the hex-encoded SHA-256 hash stored in the database.
#[must_use]
pub fn generate_session_token() -> (String, String) {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);

    let raw_hex = hex::encode(bytes);
    let hash = Sha256::digest(bytes);
    let hash_hex = hex::encode(hash);

    (raw_hex, hash_hex)
}

/// Computes the SHA-256 hash of a hex-encoded raw token.
///
/// Used during session validation and logout to convert the client's
/// raw token back to the hash stored in the database.
pub fn hash_token(raw_hex: &str) -> Result<String, hex::FromHexError> {
    let bytes = hex::decode(raw_hex)?;
    let hash = Sha256::digest(&bytes);
    Ok(hex::encode(hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_64_char_hex_token() {
        let (raw, _hash) = generate_session_token();
        assert_eq!(raw.len(), 64, "raw token should be 64 hex chars");
        assert!(hex::decode(&raw).is_ok(), "raw token should be valid hex");
    }

    #[test]
    fn generates_64_char_hex_hash() {
        let (_raw, hash) = generate_session_token();
        assert_eq!(hash.len(), 64, "hash should be 64 hex chars (SHA-256)");
        assert!(hex::decode(&hash).is_ok(), "hash should be valid hex");
    }

    #[test]
    fn raw_and_hash_differ() {
        let (raw, hash) = generate_session_token();
        assert_ne!(raw, hash);
    }

    #[test]
    fn two_tokens_are_distinct() {
        let (raw1, _) = generate_session_token();
        let (raw2, _) = generate_session_token();
        assert_ne!(raw1, raw2);
    }

    #[test]
    fn hash_token_matches_generate() {
        let (raw, expected_hash) = generate_session_token();
        let computed_hash = hash_token(&raw).expect("valid hex");
        assert_eq!(computed_hash, expected_hash);
    }

    #[test]
    fn hash_token_rejects_invalid_hex() {
        assert!(hash_token("not-valid-hex!").is_err());
    }
}
