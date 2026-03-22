//! Secret reading backend trait and implementations.
//!
//! [`SecretBackend`] provides an object-safe interface for reading decrypted
//! secrets on behalf of a plugin running in a user context.  Implementations
//! will typically delegate to `sober-crypto` for envelope decryption.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use sober_core::types::ids::UserId;
use sober_crypto::envelope::{EncryptedBlob, Mek};

/// Object-safe backend for reading decrypted secrets.
///
/// Secrets are scoped to a user — the vault implementation decides which
/// secrets a given user may access.  Returns the plaintext value on success
/// or an error string describing the failure.
pub trait SecretBackend: Send + Sync {
    /// Reads a decrypted secret by name for the given user.
    fn read_secret(
        &self,
        user_id: UserId,
        name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;
}

// ---------------------------------------------------------------------------
// PgSecretBackend
// ---------------------------------------------------------------------------

/// PostgreSQL-backed secret backend for production use.
///
/// Fetches the user's wrapped DEK from `encryption_keys`, unwraps it with the
/// MEK, then fetches and decrypts the requested secret from `secrets`.
#[derive(Clone)]
pub struct PgSecretBackend {
    pool: sqlx::PgPool,
    mek: Arc<Mek>,
}

impl PgSecretBackend {
    /// Creates a new PostgreSQL-backed secret backend.
    pub fn new(pool: sqlx::PgPool, mek: Arc<Mek>) -> Self {
        Self { pool, mek }
    }
}

impl SecretBackend for PgSecretBackend {
    fn read_secret(
        &self,
        user_id: UserId,
        name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
        let pool = self.pool.clone();
        let mek = Arc::clone(&self.mek);
        let name = name.to_owned();
        Box::pin(async move {
            // 1. Fetch the user's wrapped DEK
            let dek_row: Option<(Vec<u8>,)> =
                sqlx::query_as("SELECT encrypted_dek FROM encryption_keys WHERE user_id = $1")
                    .bind(user_id.as_uuid())
                    .fetch_optional(&pool)
                    .await
                    .map_err(|e| format!("failed to fetch DEK: {e}"))?;

            let encrypted_dek_bytes = dek_row
                .ok_or_else(|| format!("no encryption key found for user {user_id}"))?
                .0;

            // 2. Unwrap DEK with MEK
            let wrapped_blob = EncryptedBlob::from_bytes(&encrypted_dek_bytes)
                .map_err(|e| format!("invalid DEK blob: {e}"))?;
            let dek = mek
                .unwrap_dek(&wrapped_blob)
                .map_err(|e| format!("failed to unwrap DEK: {e}"))?;

            // 3. Fetch the encrypted secret by name
            let secret_row: Option<(Vec<u8>,)> = sqlx::query_as(
                "SELECT encrypted_data FROM secrets \
                 WHERE user_id = $1 AND name = $2 \
                 LIMIT 1",
            )
            .bind(user_id.as_uuid())
            .bind(&name)
            .fetch_optional(&pool)
            .await
            .map_err(|e| format!("failed to fetch secret: {e}"))?;

            let encrypted_data = secret_row
                .ok_or_else(|| format!("secret '{name}' not found"))?
                .0;

            // 4. Decrypt with DEK
            let blob = EncryptedBlob::from_bytes(&encrypted_data)
                .map_err(|e| format!("invalid secret blob: {e}"))?;
            let plaintext = dek
                .decrypt(&blob)
                .map_err(|e| format!("failed to decrypt secret: {e}"))?;

            String::from_utf8(plaintext).map_err(|e| format!("secret is not valid UTF-8: {e}"))
        })
    }
}

// ---------------------------------------------------------------------------
// Compile-time assertions
// ---------------------------------------------------------------------------

// SecretBackend is object-safe and dyn-compatible.
#[allow(dead_code)]
const _: () = {
    fn assert_object_safe(_: &dyn SecretBackend) {}
};

// Arc<dyn SecretBackend> is Send + Sync.
#[allow(dead_code)]
const _: () = {
    fn assert_send_sync<T: Send + Sync>() {}
    fn check() {
        assert_send_sync::<std::sync::Arc<dyn SecretBackend>>();
    }
};
