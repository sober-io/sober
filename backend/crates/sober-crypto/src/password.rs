//! Argon2id password hashing and verification.
//!
//! Uses OWASP-recommended parameters: 19 MiB memory, 2 iterations,
//! 1 parallelism lane. Output is a PHC-format string that embeds the
//! algorithm, parameters, salt, and hash in a single value.

use argon2::password_hash::SaltString;
use argon2::{Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version};
use rand_core::OsRng;

use crate::error::CryptoError;

/// OWASP-recommended Argon2id parameters.
const MEMORY_COST_KIB: u32 = 19_456; // 19 MiB
const TIME_COST: u32 = 2;
const PARALLELISM: u32 = 1;

/// Hash a password using Argon2id with OWASP-recommended parameters.
///
/// Returns a PHC-format string (e.g.
/// `$argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>`).
#[must_use = "the hashed password string should be stored"]
pub fn hash_password(password: &str) -> Result<String, CryptoError> {
    let salt = SaltString::generate(&mut OsRng);
    let params = Params::new(MEMORY_COST_KIB, TIME_COST, PARALLELISM, None)
        .map_err(|e| CryptoError::Hash(e.to_string()))?;
    let hasher = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let hash = hasher
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| CryptoError::Hash(e.to_string()))?;

    Ok(hash.to_string())
}

/// Verify a password against a PHC-format hash string.
///
/// Returns `true` if the password matches, `false` otherwise.
/// Uses constant-time comparison internally.
pub fn verify_password(password: &str, hash: &str) -> Result<bool, CryptoError> {
    let parsed = PasswordHash::new(hash).map_err(|e| CryptoError::Verification(e.to_string()))?;

    // Argon2::default() can verify any valid PHC string regardless of
    // the parameters used during hashing.
    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(e) => Err(CryptoError::Verification(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_succeeds() {
        let hash = hash_password("correct-horse-battery-staple").unwrap();
        assert!(verify_password("correct-horse-battery-staple", &hash).unwrap());
    }

    #[test]
    fn wrong_password_fails() {
        let hash = hash_password("correct-horse-battery-staple").unwrap();
        assert!(!verify_password("wrong-password", &hash).unwrap());
    }

    #[test]
    fn hash_is_phc_format() {
        let hash = hash_password("test-password").unwrap();
        assert!(
            hash.starts_with("$argon2id$"),
            "expected PHC format, got: {hash}"
        );
        assert!(hash.contains("m=19456"), "expected m=19456 in hash: {hash}");
        assert!(hash.contains("t=2"), "expected t=2 in hash: {hash}");
        assert!(hash.contains("p=1"), "expected p=1 in hash: {hash}");
    }

    #[test]
    fn hash_takes_measurable_time() {
        let start = std::time::Instant::now();
        let _ = hash_password("benchmark-password").unwrap();
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() > 100,
            "hashing took only {:?} — Argon2id params may be too weak",
            elapsed,
        );
    }

    #[test]
    fn invalid_hash_string_returns_error() {
        let result = verify_password("password", "not-a-valid-hash");
        assert!(result.is_err());
    }

    #[test]
    fn empty_password_hashes_and_verifies() {
        let hash = hash_password("").unwrap();
        assert!(verify_password("", &hash).unwrap());
        assert!(!verify_password("non-empty", &hash).unwrap());
    }
}
