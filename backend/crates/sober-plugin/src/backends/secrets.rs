//! Secret reading backend trait.
//!
//! [`SecretBackend`] provides an object-safe interface for reading decrypted
//! secrets on behalf of a plugin running in a user context.  Implementations
//! will typically delegate to `sober-crypto` for envelope decryption.

use std::future::Future;
use std::pin::Pin;

use sober_core::types::ids::UserId;

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
