//! Schedule deferred or recurring tasks from plugins.
//!
//! Requires the `schedule` capability in `plugin.toml`.
//!
//! # Example
//!
//! ```rust,ignore
//! use sober_pdk::schedule;
//!
//! let job_id = schedule::add("*/5 * * * *", &serde_json::json!({"task": "cleanup"}))?;
//! ```

use serde::{Deserialize, Serialize};

#[extism_pdk::host_fn("sober")]
extern "ExtismHost" {
    fn host_schedule(input: String) -> String;
}

#[derive(Serialize)]
struct ScheduleRequest {
    schedule: String,
    payload: serde_json::Value,
}

#[derive(Deserialize)]
struct ScheduleResponse {
    job_id: String,
}

fn check_error(response: &str) -> Result<(), extism_pdk::Error> {
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(response)
        && let Some(err) = obj.get("error").and_then(|e| e.as_str())
    {
        return Err(extism_pdk::Error::msg(err.to_string()));
    }
    Ok(())
}

/// Creates a scheduled job with a cron expression or interval.
///
/// Returns the job ID on success.
pub fn add(
    schedule: &str,
    payload: &serde_json::Value,
) -> Result<String, extism_pdk::Error> {
    let req = serde_json::to_string(&ScheduleRequest {
        schedule: schedule.to_string(),
        payload: payload.clone(),
    })?;

    let resp = unsafe { host_schedule(req)? };
    check_error(&resp)?;

    let parsed: ScheduleResponse = serde_json::from_str(&resp)?;
    Ok(parsed.job_id)
}
