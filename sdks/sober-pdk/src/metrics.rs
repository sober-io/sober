//! Emit metrics (counters, gauges, histograms) from plugins.
//!
//! Requires the `metrics` capability in `plugin.toml`. Metrics are forwarded
//! to the host's metrics pipeline and tagged with the plugin's identity.
//!
//! # Example
//!
//! ```rust,ignore
//! use sober_pdk::metrics;
//!
//! // Increment a counter
//! metrics::emit("requests_total", "counter", 1.0, &[("method", "GET")])?;
//!
//! // Set a gauge
//! metrics::emit("queue_depth", "gauge", 42.0, &[])?;
//!
//! // Record a histogram sample
//! metrics::emit("response_time_ms", "histogram", 23.5, &[("endpoint", "/api")])?;
//! ```

use std::collections::HashMap;

use serde::Serialize;

// ---------------------------------------------------------------------------
// Host function declaration
// ---------------------------------------------------------------------------

#[extism_pdk::host_fn("sober")]
extern "ExtismHost" {
    fn host_emit_metric(input: String) -> String;
}

// ---------------------------------------------------------------------------
// Request type (must match host_fns.rs on the host side)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct EmitMetricRequest {
    name: String,
    kind: String,
    value: f64,
    labels: HashMap<String, String>,
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

/// Emits a metric sample to the host's metrics pipeline.
///
/// # Arguments
///
/// * `name` — Metric name (e.g. `"requests_total"`).
/// * `kind` — Metric type: `"counter"`, `"gauge"`, or `"histogram"`.
/// * `value` — The numeric value to record.
/// * `labels` — Additional key-value labels as `(name, value)` pairs.
///
/// # Errors
///
/// Returns an error if the metrics capability is denied or the backing
/// metrics pipeline is not yet connected.
pub fn emit(
    name: &str,
    kind: &str,
    value: f64,
    labels: &[(&str, &str)],
) -> Result<(), extism_pdk::Error> {
    let label_map: HashMap<String, String> = labels
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();

    let req = serde_json::to_string(&EmitMetricRequest {
        name: name.to_string(),
        kind: kind.to_string(),
        value,
        labels: label_map,
    })?;

    // Safety: calling into the host-provided `host_emit_metric` function.
    let resp = unsafe { host_emit_metric(req)? };
    check_error(&resp)?;

    Ok(())
}
