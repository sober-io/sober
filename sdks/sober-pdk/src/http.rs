//! HTTP client for outbound requests from plugins.
//!
//! Requires the `network` capability in `plugin.toml`. The host enforces
//! domain allowlists declared in the manifest.
//!
//! # Example
//!
//! ```rust,ignore
//! use sober_pdk::http;
//!
//! let resp = http::get("https://api.example.com/data", &[])?;
//! assert_eq!(resp.status, 200);
//!
//! let resp = http::post(
//!     "https://api.example.com/submit",
//!     &[("Content-Type", "application/json")],
//!     r#"{"key": "value"}"#,
//! )?;
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Host function declaration
// ---------------------------------------------------------------------------

#[extism_pdk::host_fn("sober")]
extern "ExtismHost" {
    fn host_http_request(input: String) -> String;
}

// ---------------------------------------------------------------------------
// Request / response types (must match host_fns.rs on the host side)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct HttpRequestPayload {
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Option<String>,
}

/// Response returned by HTTP operations.
#[derive(Debug, Clone, Deserialize)]
pub struct HttpResponse {
    /// HTTP status code (e.g. 200, 404, 500).
    pub status: u16,
    /// Response headers from the server.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Response body as a string.
    pub body: String,
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

/// Sends an HTTP GET request.
///
/// # Arguments
///
/// * `url` — The URL to request.
/// * `headers` — Additional request headers as `(name, value)` pairs.
pub fn get(url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, extism_pdk::Error> {
    request("GET", url, headers, None)
}

/// Sends an HTTP POST request.
///
/// # Arguments
///
/// * `url` — The URL to request.
/// * `headers` — Additional request headers as `(name, value)` pairs.
/// * `body` — The request body string.
pub fn post(
    url: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Result<HttpResponse, extism_pdk::Error> {
    request("POST", url, headers, Some(body))
}

/// Sends an HTTP request with an arbitrary method.
///
/// # Arguments
///
/// * `method` — HTTP method (e.g. `"GET"`, `"POST"`, `"PUT"`, `"DELETE"`).
/// * `url` — The URL to request.
/// * `headers` — Additional request headers as `(name, value)` pairs.
/// * `body` — Optional request body string.
pub fn request(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> Result<HttpResponse, extism_pdk::Error> {
    let header_map: HashMap<String, String> = headers
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();

    let payload = HttpRequestPayload {
        method: method.to_string(),
        url: url.to_string(),
        headers: header_map,
        body: body.map(String::from),
    };

    let req = serde_json::to_string(&payload)?;

    // Safety: calling into the host-provided `host_http_request` function.
    let resp = unsafe { host_http_request(req)? };
    check_error(&resp)?;

    let parsed: HttpResponse = serde_json::from_str(&resp)?;
    Ok(parsed)
}
