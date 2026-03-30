//! Plugin generation pipeline: template scaffolding, WASM compilation, and LLM-powered generation.
//!
//! # Modules
//!
//! - `scaffold` — Template scaffolding (no LLM); generates plugin skeleton files from a manifest.
//! - `compile` — WASM compilation; shells out to `cargo build --target wasm32-wasip1`.
//! - `generate` — LLM-powered generation with a self-correcting retry loop.

pub mod compile;
pub mod error;
pub mod generate;
pub mod scaffold;

pub use compile::compile;
pub use error::GenError;
pub use generate::{GeneratedPlugin, PluginGenerator, cargo_toml};
pub use scaffold::scaffold;

/// Version requirement for the published `sober-pdk` crate used in generated plugin templates.
pub const PDK_VERSION_REQ: &str = "0.1";
