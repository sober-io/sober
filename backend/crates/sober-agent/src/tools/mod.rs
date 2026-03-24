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
//! - [`GeneratePluginTool`](generate_plugin::GeneratePluginTool) — generate plugins via LLM

pub mod artifacts;
pub mod bootstrap;
pub mod fetch_url;
pub mod generate_plugin;
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
pub use bootstrap::{
    MemoryToolConfig, SearchToolConfig, ShellToolConfig, ToolBootstrap, TurnContext,
};
pub use fetch_url::FetchUrlTool;
pub use generate_plugin::{GeneratePluginConfig, GeneratePluginTool};
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

/// User-Agent string for outbound HTTP requests.
///
/// Mimics a real browser to avoid being blocked by sites that reject bots,
/// with a Sõber identifier appended for transparency.
pub(crate) const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (X11; Linux x86_64; rv:137.0) Gecko/20100101 Firefox/137.0 Sober/",
    env!("CARGO_PKG_VERSION"),
);

/// Builds a [`reqwest::Client`] with browser-like default headers.
pub(crate) fn http_client() -> reqwest::Client {
    use reqwest::header::{self, HeaderMap, HeaderValue};

    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,application/json;q=0.8,*/*;q=0.7"));
    headers.insert(
        header::ACCEPT_LANGUAGE,
        HeaderValue::from_static("en-US,en;q=0.5"),
    );
    headers.insert(
        header::ACCEPT_ENCODING,
        HeaderValue::from_static("gzip, deflate, br"),
    );

    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .default_headers(headers)
        .build()
        .expect("failed to build HTTP client")
}
