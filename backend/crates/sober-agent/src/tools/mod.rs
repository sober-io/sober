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
//! - [`ProposeToolTool`](propose_tool::ProposeToolTool) — propose new plugin evolutions
//! - [`ProposeSkillTool`](propose_skill::ProposeSkillTool) — propose new skill evolutions
//! - [`ProposeInstructionTool`](propose_instruction::ProposeInstructionTool) — propose instruction changes
//! - [`ProposeAutomationTool`](propose_automation::ProposeAutomationTool) — propose automation evolutions

pub mod artifacts;
pub mod bootstrap;
pub mod fetch_url;
pub mod generate_plugin;
pub mod memory;
pub mod propose_automation;
pub mod propose_instruction;
pub mod propose_skill;
pub mod propose_tool;
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
pub use propose_automation::ProposeAutomationTool;
pub use propose_instruction::ProposeInstructionTool;
pub use propose_skill::ProposeSkillTool;
pub use propose_tool::ProposeToolTool;
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

// ---------------------------------------------------------------------------
// Shared helpers for evolution propose_* tools
// ---------------------------------------------------------------------------

/// Parses an optional `UserId` from a JSON field.
///
/// Returns `Ok(None)` if the field is absent or null, `Ok(Some(id))` if valid,
/// or `Err(ToolError::InvalidInput)` if the string is not a valid UUID.
fn parse_optional_user_id(
    input: &serde_json::Value,
    field: &str,
) -> Result<Option<sober_core::types::ids::UserId>, sober_core::types::tool::ToolError> {
    use sober_core::types::tool::ToolError;

    match input.get(field).and_then(|v| v.as_str()) {
        Some(s) => {
            let uuid = uuid::Uuid::parse_str(s)
                .map_err(|e| ToolError::InvalidInput(format!("invalid {field} UUID: {e}")))?;
            Ok(Some(sober_core::types::ids::UserId::from_uuid(uuid)))
        }
        None => Ok(None),
    }
}

/// Determines the initial [`EvolutionStatus`] for a new evolution event.
///
/// - `AutonomyLevel::Auto` → `Approved` (if within the daily auto-approve limit),
///   otherwise falls back to `Proposed`.
/// - `AutonomyLevel::ApprovalRequired` → `Proposed`.
/// - `AutonomyLevel::Disabled` should be handled before calling this function.
async fn resolve_initial_status<R: sober_core::types::AgentRepos>(
    autonomy: sober_core::types::enums::AutonomyLevel,
    repos: &R,
    daily_limit: i64,
) -> Result<sober_core::types::enums::EvolutionStatus, sober_core::types::tool::ToolError> {
    use sober_core::types::enums::{AutonomyLevel, EvolutionStatus};
    use sober_core::types::repo::EvolutionRepo;
    use sober_core::types::tool::ToolError;

    match autonomy {
        AutonomyLevel::Auto => {
            let count = repos
                .evolution()
                .count_auto_approved_today()
                .await
                .map_err(|e| {
                    ToolError::ExecutionFailed(format!("failed to check auto-approve count: {e}"))
                })?;
            if count < daily_limit {
                Ok(EvolutionStatus::Approved)
            } else {
                // Rate limit exceeded — fall back to requiring approval.
                Ok(EvolutionStatus::Proposed)
            }
        }
        AutonomyLevel::ApprovalRequired => Ok(EvolutionStatus::Proposed),
        AutonomyLevel::Disabled => Err(ToolError::ExecutionFailed(
            "evolution type is disabled".into(),
        )),
    }
}

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
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .default_headers(headers)
        .build()
        .expect("failed to build HTTP client")
}
