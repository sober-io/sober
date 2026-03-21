//! Key-value storage for persistent plugin state.
//!
//! Requires the `key_value` capability in `plugin.toml`.
//!
//! Values are JSON — any type that can be represented as `serde_json::Value`
//! can be stored and retrieved. Keys are scoped to the plugin instance.
//!
//! # Example
//!
//! ```rust,ignore
//! use sober_pdk::kv;
//!
//! kv::set("counter", &serde_json::json!(42))?;
//! let val = kv::get("counter")?;
//! assert_eq!(val, Some(serde_json::json!(42)));
//!
//! let keys = kv::list(Some("counter"))?;
//! kv::delete("counter")?;
//! ```

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Host function declaration
// ---------------------------------------------------------------------------

#[extism_pdk::host_fn("sober")]
extern "ExtismHost" {
    fn host_kv_get(input: String) -> String;
    fn host_kv_set(input: String) -> String;
    fn host_kv_delete(input: String) -> String;
    fn host_kv_list(input: String) -> String;
}

// ---------------------------------------------------------------------------
// Request / response types (must match host_fns.rs on the host side)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct KvGetRequest {
    key: String,
}

#[derive(Deserialize)]
struct KvGetResponse {
    value: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct KvSetRequest {
    key: String,
    value: serde_json::Value,
}

#[derive(Serialize)]
struct KvDeleteRequest {
    key: String,
}

#[derive(Serialize)]
struct KvListRequest {
    prefix: Option<String>,
}

#[derive(Deserialize)]
struct KvListResponse {
    keys: Vec<String>,
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

/// Retrieves a value by key from the plugin's key-value store.
///
/// Returns `Ok(None)` if the key does not exist.
pub fn get(key: &str) -> Result<Option<serde_json::Value>, extism_pdk::Error> {
    let req = serde_json::to_string(&KvGetRequest {
        key: key.to_string(),
    })?;

    // Safety: calling into the host-provided `host_kv_get` function.
    let resp = unsafe { host_kv_get(req)? };
    check_error(&resp)?;

    let parsed: KvGetResponse = serde_json::from_str(&resp)?;
    Ok(parsed.value)
}

/// Stores a value under the given key in the plugin's key-value store.
///
/// Overwrites any existing value for the key.
pub fn set(key: &str, value: &serde_json::Value) -> Result<(), extism_pdk::Error> {
    let req = serde_json::to_string(&KvSetRequest {
        key: key.to_string(),
        value: value.clone(),
    })?;

    // Safety: calling into the host-provided `host_kv_set` function.
    let resp = unsafe { host_kv_set(req)? };
    check_error(&resp)?;

    Ok(())
}

/// Deletes a key from the plugin's key-value store.
///
/// No error is returned if the key does not exist.
pub fn delete(key: &str) -> Result<(), extism_pdk::Error> {
    let req = serde_json::to_string(&KvDeleteRequest {
        key: key.to_string(),
    })?;

    // Safety: calling into the host-provided `host_kv_delete` function.
    let resp = unsafe { host_kv_delete(req)? };
    check_error(&resp)?;

    Ok(())
}

/// Lists keys in the plugin's key-value store, optionally filtered by prefix.
///
/// Pass `None` to list all keys. Pass `Some("prefix:")` to list only keys
/// starting with `"prefix:"`.
pub fn list(prefix: Option<&str>) -> Result<Vec<String>, extism_pdk::Error> {
    let req = serde_json::to_string(&KvListRequest {
        prefix: prefix.map(String::from),
    })?;

    // Safety: calling into the host-provided `host_kv_list` function.
    let resp = unsafe { host_kv_list(req)? };
    check_error(&resp)?;

    let parsed: KvListResponse = serde_json::from_str(&resp)?;
    Ok(parsed.keys)
}
