//! Invoke other tools registered in the Sober runtime.
//!
//! Requires the `tool_call` capability in `plugin.toml`. The host enforces
//! the tool allowlist declared in the manifest.
//!
//! # Example
//!
//! ```rust,ignore
//! use sober_pdk::tool;
//!
//! let result = tool::call("web_search", serde_json::json!({ "query": "Rust WASM" }))?;
//! let items = result["results"].as_array();
//! ```

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Host function declaration
// ---------------------------------------------------------------------------

#[extism_pdk::host_fn("sober")]
extern "ExtismHost" {
    fn host_call_tool(input: String) -> String;
}

// ---------------------------------------------------------------------------
// Request / response types (must match host_fns.rs on the host side)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct CallToolRequest {
    tool: String,
    input: serde_json::Value,
}

#[derive(Deserialize)]
struct CallToolResponse {
    output: serde_json::Value,
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

/// Invokes a named tool with the given JSON input and returns its output.
///
/// Tools are other plugins or built-in capabilities registered in the
/// Sober runtime. The host routes the call, enforces capability checks,
/// and returns the tool's JSON output.
///
/// # Arguments
///
/// * `tool_name` — The registered name of the tool to invoke.
/// * `input` — Arbitrary JSON input to pass to the tool.
///
/// # Errors
///
/// Returns an error if the tool does not exist, access is denied, or
/// the tool execution fails.
pub fn call(
    tool_name: &str,
    input: serde_json::Value,
) -> Result<serde_json::Value, extism_pdk::Error> {
    let req = serde_json::to_string(&CallToolRequest {
        tool: tool_name.to_string(),
        input,
    })?;

    // Safety: calling into the host-provided `host_call_tool` function.
    let resp = unsafe { host_call_tool(req)? };
    check_error(&resp)?;

    let parsed: CallToolResponse = serde_json::from_str(&resp)?;
    Ok(parsed.output)
}
