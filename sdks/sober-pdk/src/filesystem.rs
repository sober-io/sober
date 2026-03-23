//! Sandboxed filesystem access for plugins.
//!
//! Requires the `filesystem` capability in `plugin.toml`.
//!
//! # Example
//!
//! ```rust,ignore
//! use sober_pdk::fs;
//!
//! fs::write("/workspace/data/output.txt", "hello")?;
//! let content = fs::read("/workspace/data/output.txt")?;
//! ```

use serde::{Deserialize, Serialize};

#[extism_pdk::host_fn("sober")]
extern "ExtismHost" {
    fn host_fs_read(input: String) -> String;
    fn host_fs_write(input: String) -> String;
}

#[derive(Serialize)]
struct FsReadRequest {
    path: String,
}

#[derive(Serialize)]
struct FsWriteRequest {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct FsReadResponse {
    content: String,
}

fn check_error(response: &str) -> Result<(), extism_pdk::Error> {
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(response)
        && let Some(err) = obj.get("error").and_then(|e| e.as_str())
    {
        return Err(extism_pdk::Error::msg(err.to_string()));
    }
    Ok(())
}

/// Reads a file from the sandboxed filesystem.
pub fn read(path: &str) -> Result<String, extism_pdk::Error> {
    let req = serde_json::to_string(&FsReadRequest {
        path: path.to_string(),
    })?;

    let resp = unsafe { host_fs_read(req)? };
    check_error(&resp)?;

    let parsed: FsReadResponse = serde_json::from_str(&resp)?;
    Ok(parsed.content)
}

/// Writes content to a file in the sandboxed filesystem.
pub fn write(path: &str, content: &str) -> Result<(), extism_pdk::Error> {
    let req = serde_json::to_string(&FsWriteRequest {
        path: path.to_string(),
        content: content.to_string(),
    })?;

    let resp = unsafe { host_fs_write(req)? };
    check_error(&resp)?;
    Ok(())
}
