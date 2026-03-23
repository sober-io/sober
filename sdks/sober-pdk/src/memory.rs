//! Read and write access to the Sober memory system.
//!
//! Requires the `memory_read` or `memory_write` capability in `plugin.toml`.
//!
//! # Example
//!
//! ```rust,ignore
//! use sober_pdk::memory;
//!
//! let hits = memory::query("recent meetings", None, Some(5))?;
//! memory::write("User prefers dark mode", None, Default::default())?;
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[extism_pdk::host_fn("sober")]
extern "ExtismHost" {
    fn host_memory_query(input: String) -> String;
    fn host_memory_write(input: String) -> String;
}

#[derive(Serialize)]
struct MemoryQueryRequest {
    query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<u32>,
}

#[derive(Serialize)]
struct MemoryWriteRequest {
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
    #[serde(default)]
    metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryHit {
    pub content: String,
    pub score: f64,
    pub chunk_type: Option<String>,
}

#[derive(Deserialize)]
struct MemoryQueryResponse {
    results: Vec<MemoryHit>,
}

fn check_error(response: &str) -> Result<(), extism_pdk::Error> {
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(response)
        && let Some(err) = obj.get("error").and_then(|e| e.as_str())
    {
        return Err(extism_pdk::Error::msg(err.to_string()));
    }
    Ok(())
}

/// Searches the memory system for relevant chunks.
pub fn query(
    query: &str,
    scope: Option<&str>,
    limit: Option<u32>,
) -> Result<Vec<MemoryHit>, extism_pdk::Error> {
    let req = serde_json::to_string(&MemoryQueryRequest {
        query: query.to_string(),
        scope: scope.map(String::from),
        limit,
    })?;

    let resp = unsafe { host_memory_query(req)? };
    check_error(&resp)?;

    let parsed: MemoryQueryResponse = serde_json::from_str(&resp)?;
    Ok(parsed.results)
}

/// Writes a chunk to the memory system.
pub fn write(
    content: &str,
    scope: Option<&str>,
    metadata: HashMap<String, serde_json::Value>,
) -> Result<(), extism_pdk::Error> {
    let req = serde_json::to_string(&MemoryWriteRequest {
        content: content.to_string(),
        scope: scope.map(String::from),
        metadata,
    })?;

    let resp = unsafe { host_memory_write(req)? };
    check_error(&resp)?;
    Ok(())
}
