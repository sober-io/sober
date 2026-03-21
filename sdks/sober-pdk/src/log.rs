//! Logging utilities for Sober plugins.
//!
//! Always available — not gated behind any feature flag. Log messages are
//! forwarded to the host's `tracing` infrastructure and tagged with the
//! plugin's identity.
//!
//! # Example
//!
//! ```rust,ignore
//! sober_pdk::log::info("starting up");
//! sober_pdk::log::debug("processing item");
//! ```

use std::collections::HashMap;

use serde::Serialize;

/// JSON payload sent to the `host_log` host function.
#[derive(Serialize)]
struct LogRequest {
    level: String,
    message: String,
    fields: HashMap<String, serde_json::Value>,
}

#[extism_pdk::host_fn("sober")]
extern "ExtismHost" {
    fn host_log(input: String) -> String;
}

/// Sends a log message at the given level.
///
/// Serialization failures are silently ignored — logging should never crash
/// the plugin.
fn log_at(level: &str, msg: &str) {
    let req = LogRequest {
        level: level.to_string(),
        message: msg.to_string(),
        fields: HashMap::new(),
    };
    if let Ok(json) = serde_json::to_string(&req) {
        // Safety: calling into the host-provided `host_log` function.
        unsafe {
            let _ = host_log(json);
        }
    }
}

/// Sends a log message at the given level with structured fields.
///
/// Serialization failures are silently ignored.
fn log_at_with_fields(level: &str, msg: &str, fields: HashMap<String, serde_json::Value>) {
    let req = LogRequest {
        level: level.to_string(),
        message: msg.to_string(),
        fields,
    };
    if let Ok(json) = serde_json::to_string(&req) {
        // Safety: calling into the host-provided `host_log` function.
        unsafe {
            let _ = host_log(json);
        }
    }
}

/// Logs a message at the **info** level.
pub fn info(msg: &str) {
    log_at("info", msg);
}

/// Logs a message at the **warn** level.
pub fn warn(msg: &str) {
    log_at("warn", msg);
}

/// Logs a message at the **error** level.
pub fn error(msg: &str) {
    log_at("error", msg);
}

/// Logs a message at the **debug** level.
pub fn debug(msg: &str) {
    log_at("debug", msg);
}

/// Logs a message at the **trace** level.
pub fn trace(msg: &str) {
    log_at("trace", msg);
}

/// Logs a message at the **info** level with structured fields.
pub fn info_with(msg: &str, fields: HashMap<String, serde_json::Value>) {
    log_at_with_fields("info", msg, fields);
}

/// Logs a message at the **warn** level with structured fields.
pub fn warn_with(msg: &str, fields: HashMap<String, serde_json::Value>) {
    log_at_with_fields("warn", msg, fields);
}

/// Logs a message at the **error** level with structured fields.
pub fn error_with(msg: &str, fields: HashMap<String, serde_json::Value>) {
    log_at_with_fields("error", msg, fields);
}

/// Logs a message at the **debug** level with structured fields.
pub fn debug_with(msg: &str, fields: HashMap<String, serde_json::Value>) {
    log_at_with_fields("debug", msg, fields);
}

/// Logs a message at the **trace** level with structured fields.
pub fn trace_with(msg: &str, fields: HashMap<String, serde_json::Value>) {
    log_at_with_fields("trace", msg, fields);
}
