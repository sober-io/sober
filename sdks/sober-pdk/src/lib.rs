//! # Sober Plugin Development Kit (PDK)
//!
//! Guest-side SDK for writing Sober plugins as WebAssembly modules.
//!
//! All capability modules are always compiled. Enforcement happens at two
//! levels instead of compile-time feature gating:
//!
//! - **Load-time**: the host only wires host functions for capabilities
//!   declared in `plugin.toml`. Calling an undeclared capability's host
//!   function fails at WASM instantiation (unresolved import).
//! - **Runtime**: host functions check granted capabilities and return
//!   `CapabilityDenied` if the plugin wasn't granted access.
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
pub mod http;

/// Key-value storage for persistent plugin state.
pub mod kv;

/// Read-only access to configured secrets.
pub mod secret;

/// Invoke other tools registered in the runtime.
pub mod tool;

/// Emit metrics (counters, gauges, histograms).
pub mod metrics;

/// Read and write access to the Sober memory system.
pub mod memory;

/// Read access to conversation context.
pub mod conversation;

/// Schedule deferred or recurring tasks.
pub mod schedule;

/// Sandboxed filesystem access.
pub mod filesystem;
/// Shorthand alias for [`filesystem`].
pub use filesystem as fs;

/// Invoke LLM completions from within a plugin.
pub mod llm;
