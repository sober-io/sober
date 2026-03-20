//! Unified plugin system — registry, audit, manifest, and capability types.
//!
//! Manages all three plugin kinds (MCP, Skill, WASM) under one registry
//! with type-aware lifecycle and audit pipeline.

pub mod audit;
pub mod capability;
pub mod error;
pub mod manifest;
pub mod registry;

// pub use audit::{AuditPipeline, AuditReport, AuditVerdict, StageResult};
pub use capability::{Cap, CapabilitiesConfig, Capability};
pub use error::PluginError;
// pub use manifest::PluginManifest;
// pub use registry::PluginRegistry;
