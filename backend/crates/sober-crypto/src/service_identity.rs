//! Ed25519-based service identity tokens for gRPC inter-service authentication.
//!
//! Each service holds an [`ServiceIdentity`] containing a name and Ed25519 keypair.
//! When calling another service via gRPC, it signs a token containing its name
//! and a timestamp. The receiving service verifies the signature and checks the
//! caller name against an allowlist.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::CryptoError;
use crate::keys::{self, SigningKey, VerifyingKey};

/// Default token TTL in seconds (5 minutes).
const DEFAULT_TTL_SECS: u64 = 300;

/// A service's cryptographic identity for gRPC authentication.
pub struct ServiceIdentity {
    name: String,
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

impl ServiceIdentity {
    /// Create a new service identity with a fresh Ed25519 keypair.
    pub fn generate(name: impl Into<String>) -> Self {
        let (signing_key, verifying_key) = keys::generate_keypair();
        Self {
            name: name.into(),
            signing_key,
            verifying_key,
        }
    }

    /// Create a service identity from an existing keypair.
    pub fn from_keypair(
        name: impl Into<String>,
        signing_key: SigningKey,
        verifying_key: VerifyingKey,
    ) -> Self {
        Self {
            name: name.into(),
            signing_key,
            verifying_key,
        }
    }

    /// The service name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The public (verifying) key for this service.
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }

    /// Sign a token containing the service name and current timestamp.
    ///
    /// Token format: `<service_name>:<unix_timestamp_secs>`
    /// The signature covers this token string.
    pub fn sign_token(&self) -> ServiceToken {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_secs();
        let token_data = format!("{}:{}", self.name, timestamp);
        let signature = keys::sign(&self.signing_key, token_data.as_bytes());

        ServiceToken {
            service_name: self.name.clone(),
            timestamp,
            signature_hex: hex::encode(signature.to_bytes()),
        }
    }
}

/// A signed service identity token transmitted as gRPC metadata.
#[derive(Debug, Clone)]
pub struct ServiceToken {
    /// The claimed service name.
    pub service_name: String,
    /// Unix timestamp (seconds) when the token was created.
    pub timestamp: u64,
    /// Hex-encoded Ed25519 signature over `"<service_name>:<timestamp>"`.
    pub signature_hex: String,
}

impl ServiceToken {
    /// Serialize to a wire format string: `<service_name>:<timestamp>:<signature_hex>`.
    pub fn encode(&self) -> String {
        format!(
            "{}:{}:{}",
            self.service_name, self.timestamp, self.signature_hex
        )
    }

    /// Parse a token from its wire format.
    pub fn decode(s: &str) -> Result<Self, CryptoError> {
        // Format: name:timestamp:signature_hex
        // The name itself could theoretically contain colons, so split from
        // the right to get the signature and timestamp first.
        let parts: Vec<&str> = s.rsplitn(3, ':').collect();
        if parts.len() != 3 {
            return Err(CryptoError::InvalidData(
                "invalid service token format".into(),
            ));
        }
        // rsplitn reverses order: [signature, timestamp, name]
        let signature_hex = parts[0].to_string();
        let timestamp: u64 = parts[1]
            .parse()
            .map_err(|_| CryptoError::InvalidData("invalid timestamp in service token".into()))?;
        let service_name = parts[2].to_string();

        Ok(Self {
            service_name,
            timestamp,
            signature_hex,
        })
    }
}

/// Verify a service token against a known verifying key.
///
/// Checks:
/// 1. Signature is valid for the token data.
/// 2. Token is not expired (within `ttl_secs`, default 5 minutes).
/// 3. Service name matches `expected_service` (if provided).
pub fn verify_token(
    token: &ServiceToken,
    verifying_key: &VerifyingKey,
    expected_service: Option<&str>,
    ttl_secs: Option<u64>,
) -> Result<(), CryptoError> {
    // Check service name if expected
    if let Some(expected) = expected_service
        && token.service_name != expected
    {
        return Err(CryptoError::Signature(format!(
            "service name mismatch: expected '{}', got '{}'",
            expected, token.service_name
        )));
    }

    // Check TTL
    let ttl = ttl_secs.unwrap_or(DEFAULT_TTL_SECS);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs();
    let age = now.saturating_sub(token.timestamp);
    if age > ttl {
        return Err(CryptoError::Signature(format!(
            "service token expired: age {}s exceeds TTL {}s",
            age, ttl
        )));
    }

    // Verify signature
    let token_data = format!("{}:{}", token.service_name, token.timestamp);
    let sig_bytes = hex::decode(&token.signature_hex)
        .map_err(|e| CryptoError::InvalidData(format!("invalid signature hex: {e}")))?;
    let signature = ed25519_dalek::Signature::from_slice(&sig_bytes)
        .map_err(|e| CryptoError::Signature(format!("invalid signature bytes: {e}")))?;

    keys::verify(verifying_key, token_data.as_bytes(), &signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_roundtrip() {
        let identity = ServiceIdentity::generate("scheduler");
        let token = identity.sign_token();

        assert_eq!(token.service_name, "scheduler");
        assert!(verify_token(&token, identity.verifying_key(), Some("scheduler"), None).is_ok());
    }

    #[test]
    fn wrong_service_name_rejected() {
        let identity = ServiceIdentity::generate("scheduler");
        let token = identity.sign_token();

        let result = verify_token(&token, identity.verifying_key(), Some("agent"), None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("service name mismatch")
        );
    }

    #[test]
    fn wrong_key_rejected() {
        let identity = ServiceIdentity::generate("scheduler");
        let other = ServiceIdentity::generate("other");
        let token = identity.sign_token();

        let result = verify_token(&token, other.verifying_key(), None, None);
        assert!(result.is_err());
    }

    #[test]
    fn expired_token_rejected() {
        let identity = ServiceIdentity::generate("scheduler");
        let mut token = identity.sign_token();
        // Set timestamp to 10 minutes ago
        token.timestamp -= 600;
        // Re-sign with the old timestamp won't work — but we can test
        // by just checking TTL against the tampered timestamp.
        // The signature will be invalid anyway, but TTL check comes first.
        let result = verify_token(&token, identity.verifying_key(), None, Some(60));
        assert!(result.is_err());
    }

    #[test]
    fn token_encode_decode_roundtrip() {
        let identity = ServiceIdentity::generate("api-gateway");
        let token = identity.sign_token();
        let encoded = token.encode();
        let decoded = ServiceToken::decode(&encoded).expect("decode should succeed");

        assert_eq!(decoded.service_name, "api-gateway");
        assert_eq!(decoded.timestamp, token.timestamp);
        assert_eq!(decoded.signature_hex, token.signature_hex);

        // Decoded token should still verify
        assert!(
            verify_token(
                &decoded,
                identity.verifying_key(),
                Some("api-gateway"),
                None
            )
            .is_ok()
        );
    }

    #[test]
    fn no_expected_service_accepts_any_name() {
        let identity = ServiceIdentity::generate("scheduler");
        let token = identity.sign_token();

        // None means don't check service name
        assert!(verify_token(&token, identity.verifying_key(), None, None).is_ok());
    }

    #[test]
    fn invalid_token_format_rejected() {
        assert!(ServiceToken::decode("invalid").is_err());
        assert!(ServiceToken::decode("only:two").is_err());
    }
}
