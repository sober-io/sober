//! # Sober Plugin Development Kit (PDK)
//!
//! Guest-side SDK for writing Sober plugins as WebAssembly modules.
//!
//! Modules are gated behind Cargo features that correspond to capabilities
//! declared in the plugin's `plugin.toml` manifest. A `build.rs` in each
//! plugin project reads the manifest and enables the matching features
//! automatically — plugin authors never set features by hand.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use sober_pdk::{plugin_fn, FnResult, Json};
//!
//! #[plugin_fn]
//! pub fn handle(input: Json<serde_json::Value>) -> FnResult<Json<serde_json::Value>> {
//!     let name = input.0["name"].as_str().unwrap_or("world");
//!     Ok(Json(serde_json::json!({ "greeting": format!("Hello, {name}!") })))
//! }
//! ```

// Re-export core extism-pdk types that every plugin uses.
pub use extism_pdk::{self, plugin_fn, FnResult, FromBytes, Json, ToBytes};

/// Logging utilities — always available.
pub mod log;

/// HTTP client for outbound network requests.
#[cfg(feature = "network")]
pub mod http;

/// Key-value storage for persistent plugin state.
#[cfg(feature = "key_value")]
pub mod kv;

/// Read-only access to configured secrets.
#[cfg(feature = "secret_read")]
pub mod secret;

/// Invoke other tools registered in the runtime.
#[cfg(feature = "tool_call")]
pub mod tool;

/// Emit metrics (counters, gauges, histograms).
#[cfg(feature = "metrics")]
pub mod metrics;

/// Read and write access to the Sober memory system.
///
/// Enabled by either `memory_read` or `memory_write`.
#[cfg(any(feature = "memory_read", feature = "memory_write"))]
pub mod memory;

/// Read access to conversation context.
#[cfg(feature = "conversation_read")]
pub mod conversation;

/// Schedule deferred or recurring tasks.
#[cfg(feature = "schedule")]
pub mod schedule;

/// Sandboxed filesystem access.
#[cfg(feature = "filesystem")]
pub mod filesystem;

/// Invoke LLM completions from within a plugin.
#[cfg(feature = "llm_call")]
pub mod llm;
