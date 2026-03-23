//! Invoke LLM completions from within a plugin.
//!
//! Requires the `llm_inference` capability in `plugin.toml`.
//!
//! # Example
//!
//! ```rust,ignore
//! use sober_pdk::llm;
//!
//! let response = llm::complete("Summarize this text: ...", None, None)?;
//! ```

use serde::{Deserialize, Serialize};

#[extism_pdk::host_fn("sober")]
extern "ExtismHost" {
    fn host_llm_complete(input: String) -> String;
}

#[derive(Serialize)]
struct LlmCompleteRequest {
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    raw: bool,
}

#[derive(Deserialize)]
struct LlmCompleteResponse {
    text: String,
}

fn check_error(response: &str) -> Result<(), extism_pdk::Error> {
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(response)
        && let Some(err) = obj.get("error").and_then(|e| e.as_str())
    {
        return Err(extism_pdk::Error::msg(err.to_string()));
    }
    Ok(())
}

/// Sends a prompt to the LLM and returns the completion text.
///
/// By default the agent's system prompt is included for consistent behavior.
/// `model` and `max_tokens` are optional — the host uses defaults if omitted.
pub fn complete(
    prompt: &str,
    model: Option<&str>,
    max_tokens: Option<u32>,
) -> Result<String, extism_pdk::Error> {
    let req = serde_json::to_string(&LlmCompleteRequest {
        prompt: prompt.to_string(),
        model: model.map(String::from),
        max_tokens,
        raw: false,
    })?;

    let resp = unsafe { host_llm_complete(req)? };
    check_error(&resp)?;

    let parsed: LlmCompleteResponse = serde_json::from_str(&resp)?;
    Ok(parsed.text)
}

/// Sends a raw prompt to the LLM without the agent's system prompt.
///
/// Use this when the plugin needs full control over the conversation context.
pub fn complete_raw(
    prompt: &str,
    model: Option<&str>,
    max_tokens: Option<u32>,
) -> Result<String, extism_pdk::Error> {
    let req = serde_json::to_string(&LlmCompleteRequest {
        prompt: prompt.to_string(),
        model: model.map(String::from),
        max_tokens,
        raw: true,
    })?;

    let resp = unsafe { host_llm_complete(req)? };
    check_error(&resp)?;

    let parsed: LlmCompleteResponse = serde_json::from_str(&resp)?;
    Ok(parsed.text)
}
