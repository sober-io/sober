//! Read-only access to secrets configured for the plugin.
//!
//! Requires the `secret_read` capability in `plugin.toml`. Secrets are
//! managed by the host and decrypted on-demand — the plugin never sees
//! raw encryption keys.
//!
//! # Example
//!
//! ```rust,ignore
//! use sober_pdk::secret;
//!
//! let api_key = secret::read("API_KEY")?;
//! ```

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Host function declaration
// ---------------------------------------------------------------------------

#[extism_pdk::host_fn("sober")]
extern "ExtismHost" {
    fn host_read_secret(input: String) -> String;
}

// ---------------------------------------------------------------------------
// Request / response types (must match host_fns.rs on the host side)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ReadSecretRequest {
    name: String,
}

#[derive(Deserialize)]
struct ReadSecretResponse {
    value: String,
}

// ---------------------------------------------------------------------------
// Error checking
// ---------------------------------------------------------------------------

/// Inspects a JSON response for an `"error"` field and converts it to an error.
fn check_error(response: &str) -> Result<(), extism_pdk::Error> {
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(response)
        && let Some(err) = obj.get("error").and_then(|e| e.as_str())
    {
        return Err(extism_pdk::Error::msg(err.to_string()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Reads a secret by name from the host's secret store.
///
/// Returns the secret value as a plaintext string. The host handles
/// decryption — the plugin receives the cleartext value.
///
/// # Errors
///
/// Returns an error if the secret does not exist, access is denied,
/// or the backing secret store is not yet connected.
pub fn read(name: &str) -> Result<String, extism_pdk::Error> {
    let req = serde_json::to_string(&ReadSecretRequest {
        name: name.to_string(),
    })?;

    // Safety: calling into the host-provided `host_read_secret` function.
    let resp = unsafe { host_read_secret(req)? };
    check_error(&resp)?;

    let parsed: ReadSecretResponse = serde_json::from_str(&resp)?;
    Ok(parsed.value)
}
