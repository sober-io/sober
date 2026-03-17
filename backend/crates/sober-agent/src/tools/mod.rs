//! Built-in agent tools and the tool registry.
//!
//! This module provides the [`ToolRegistry`](registry::ToolRegistry) for
//! discovering and invoking tools, along with built-in implementations:
//!
//! - [`WebSearchTool`](web_search::WebSearchTool) — web search via SearXNG
//! - [`FetchUrlTool`](fetch_url::FetchUrlTool) — fetch and extract URL content
//! - [`ShellTool`](shell::ShellTool) — sandboxed shell command execution
//! - [`RecallTool`](memory::RecallTool) — active memory search
//! - [`RememberTool`](memory::RememberTool) — explicit memory storage
//! - [`CreateArtifactTool`](artifacts::CreateArtifactTool) — create workspace artifacts
//! - [`ListArtifactsTool`](artifacts::ListArtifactsTool) — list workspace artifacts
//! - [`ReadArtifactTool`](artifacts::ReadArtifactTool) — read artifact content
//! - [`DeleteArtifactTool`](artifacts::DeleteArtifactTool) — archive artifacts
//! - [`StoreSecretTool`](secrets::StoreSecretTool) — encrypt and store a secret
//! - [`ReadSecretTool`](secrets::ReadSecretTool) — decrypt and retrieve a secret
//! - [`ListSecretsTool`](secrets::ListSecretsTool) — list secret metadata
//! - [`DeleteSecretTool`](secrets::DeleteSecretTool) — delete a secret
//! - [`CreateSnapshotTool`](snapshots::CreateSnapshotTool) — create workspace snapshot
//! - [`ListSnapshotsTool`](snapshots::ListSnapshotsTool) — list workspace snapshots
//! - [`RestoreSnapshotTool`](snapshots::RestoreSnapshotTool) — restore from snapshot

pub mod artifacts;
pub mod bootstrap;
pub mod fetch_url;
pub mod memory;
pub mod registry;
pub mod scheduler;
pub mod secrets;
pub mod shell;
pub mod snapshots;
pub mod web_search;

pub use artifacts::{
    ArtifactToolContext, CreateArtifactTool, DeleteArtifactTool, ListArtifactsTool,
    ReadArtifactTool,
};
pub use bootstrap::{ToolBootstrap, TurnContext};
pub use fetch_url::FetchUrlTool;
pub use memory::{RecallTool, RememberTool};
pub use registry::ToolRegistry;
pub use scheduler::SchedulerTools;
pub use secrets::{
    DeleteSecretTool, ListSecretsTool, ReadSecretTool, SecretToolContext, StoreSecretTool,
};
pub use shell::{SharedPermissionMode, ShellTool};
pub use snapshots::{
    CreateSnapshotTool, ListSnapshotsTool, RestoreSnapshotTool, SnapshotToolContext,
};
pub use web_search::WebSearchTool;
