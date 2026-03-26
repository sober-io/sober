//! Domain enums that map to PostgreSQL custom types.
//!
//! Each enum uses `#[serde(rename_all = "lowercase")]` for consistent JSON
//! serialization and `#[sqlx(type_name, rename_all)]` (behind the `postgres`
//! feature) for direct mapping to PostgreSQL enum types.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Memory/authorization scope level.
///
/// Maps to the `scope_kind` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "scope_kind", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum ScopeKind {
    /// Global system scope.
    System,
    /// Per-user scope.
    User,
    /// Shared group/team scope.
    Group,
    /// Ephemeral conversation scope.
    Session,
}

/// User account lifecycle status.
///
/// Maps to the `user_status` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "user_status", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum UserStatus {
    /// Account created but not yet verified.
    Pending,
    /// Active account.
    Active,
    /// Administratively disabled.
    Disabled,
}

impl fmt::Display for UserStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => f.write_str("pending"),
            Self::Active => f.write_str("active"),
            Self::Disabled => f.write_str("disabled"),
        }
    }
}

/// Author type of a message in a conversation.
///
/// Maps to the `message_role` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "message_role", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// Human user message.
    User,
    /// AI assistant response.
    Assistant,
    /// System prompt or instruction.
    System,
    /// System-generated event notification (e.g. user joined/left).
    Event,
}

/// The kind/type of a conversation.
///
/// Maps to the `conversation_kind` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "conversation_kind", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum ConversationKind {
    /// A one-on-one conversation between user and agent.
    Direct,
    /// A multi-user conversation (future).
    Group,
    /// The user's permanent catch-all inbox.
    Inbox,
}

/// The role a user holds in a conversation.
///
/// Maps to the `user_role` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "user_role", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum ConversationUserRole {
    /// Conversation creator/owner.
    Owner,
    /// Elevated participant with moderation privileges.
    Admin,
    /// Regular participant.
    Member,
}

/// Agent response mode for group conversations.
///
/// Maps to the `agent_mode` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "agent_mode", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    /// Respond to every message.
    Always,
    /// Only respond when @sober is mentioned.
    Mention,
    /// Never respond automatically.
    Silent,
}

/// Lifecycle state of a scheduled job.
///
/// Maps to the `job_status` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "job_status", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    /// Job is active and will run on schedule.
    Active,
    /// Job is temporarily paused.
    Paused,
    /// Job has been cancelled and will not run again.
    Cancelled,
    /// Job is currently being executed by the scheduler.
    Running,
}

/// How much autonomy the agent has when executing shell commands.
///
/// Maps to the `permission_mode` PostgreSQL enum.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "permission_mode", rename_all = "snake_case")
)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Every command requires explicit user approval.
    Interactive,
    /// Safe/moderate commands auto-approve; dangerous commands require approval.
    #[default]
    PolicyBased,
    /// All commands auto-approve within sandbox constraints.
    Autonomous,
}

impl PermissionMode {
    /// Returns the canonical string representation for API serialization.
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionMode::Interactive => "interactive",
            PermissionMode::PolicyBased => "policy_based",
            PermissionMode::Autonomous => "autonomous",
        }
    }
}

/// Network access mode for sandbox execution.
///
/// Maps to the `sandbox_net_mode` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "sandbox_net_mode", rename_all = "snake_case")
)]
#[serde(rename_all = "snake_case")]
pub enum SandboxNetMode {
    /// No network access (loopback only).
    None,
    /// Only specified domains are accessible.
    AllowedDomains,
    /// Full unrestricted network access.
    Full,
}

/// Lifecycle state of a workspace.
///
/// Maps to the `workspace_state` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "workspace_state", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceState {
    /// Workspace is active and usable.
    Active,
    /// Workspace is archived (soft-deleted, read-only).
    Archived,
    /// Workspace is deleted (pending cleanup).
    Deleted,
}

/// Lifecycle state of a git worktree.
///
/// Maps to the `worktree_state` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "worktree_state", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum WorktreeState {
    /// Worktree is active and in use.
    Active,
    /// Worktree has been idle too long, candidate for cleanup.
    Stale,
    /// Worktree is being removed.
    Removing,
}

/// Lifecycle state of a workspace artifact.
///
/// Maps to the `artifact_state` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "artifact_state", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactState {
    /// Artifact is a work in progress.
    Draft,
    /// Artifact has been proposed for review.
    Proposed,
    /// Artifact has been approved.
    Approved,
    /// Artifact has been rejected.
    Rejected,
    /// Artifact has been archived.
    Archived,
}

/// Relationship type between two artifacts.
///
/// Maps to the `artifact_relation` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "artifact_relation", rename_all = "snake_case")
)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRelation {
    /// Target artifact spawned from source.
    SpawnedBy,
    /// Target artifact supersedes source.
    Supersedes,
    /// Target artifact references source.
    References,
    /// Target artifact implements source (e.g. code implements a proposal).
    Implements,
}

/// Kind of workspace artifact.
///
/// Maps to the `artifact_kind` PostgreSQL enum.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "artifact_kind", rename_all = "snake_case")
)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    /// A code change (commit, diff, PR).
    CodeChange,
    /// A document (markdown, text, etc.).
    #[default]
    Document,
    /// A proposal for changes.
    Proposal,
    /// A workspace snapshot (tar archive).
    Snapshot,
    /// An execution trace (debugging).
    Trace,
}

/// The kind/type of a plugin.
///
/// Maps to the `plugin_kind` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "plugin_kind", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    /// An MCP (Model Context Protocol) server plugin.
    Mcp,
    /// A skill plugin (custom agent capability).
    Skill,
    /// A WebAssembly sandbox plugin.
    Wasm,
}

/// How a plugin was discovered/installed.
///
/// Maps to the `plugin_origin` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "plugin_origin", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum PluginOrigin {
    /// Shipped with the system.
    System,
    /// Discovered or installed by the agent autonomously.
    Agent,
    /// Installed by a user.
    User,
}

/// The scope at which a plugin is available.
///
/// Maps to the `plugin_scope` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "plugin_scope", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum PluginScope {
    /// Available system-wide.
    System,
    /// Available to a specific user.
    User,
    /// Available within a specific workspace.
    Workspace,
}

/// Lifecycle status of a plugin.
///
/// Maps to the `plugin_status` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "plugin_status", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum PluginStatus {
    /// Plugin is active and available for use.
    Enabled,
    /// Plugin is installed but not active.
    Disabled,
    /// Plugin has failed and is not usable.
    Failed,
}

/// Source of a tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "tool_execution_source", rename_all = "lowercase")
)]
pub enum ToolExecutionSource {
    /// A built-in tool provided by the agent.
    Builtin,
    /// A tool provided by a plugin.
    Plugin,
    /// A tool provided via MCP (Model Context Protocol).
    Mcp,
}

/// Status of a tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "tool_execution_status", rename_all = "lowercase")
)]
pub enum ToolExecutionStatus {
    /// Execution has been created but not yet started.
    Pending,
    /// Execution is in progress.
    Running,
    /// Execution completed successfully.
    Completed,
    /// Execution failed.
    Failed,
    /// Execution was cancelled.
    Cancelled,
}

/// Authorization role kind.
///
/// Known roles have dedicated variants for compile-time safety.
/// Custom roles are supported via [`RoleKind::Custom`] for user-defined roles.
///
/// Serializes as a lowercase string (e.g. `"user"`, `"admin"`, `"moderator"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RoleKind {
    /// Standard user role.
    User,
    /// Administrator role.
    Admin,
    /// A custom role defined at runtime.
    Custom(String),
}

impl RoleKind {
    /// Returns the string representation used in the database.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::User => "user",
            Self::Admin => "admin",
            Self::Custom(name) => name,
        }
    }
}

impl fmt::Display for RoleKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for RoleKind {
    fn from(s: String) -> Self {
        match s.as_str() {
            "user" => Self::User,
            "admin" => Self::Admin,
            _ => Self::Custom(s),
        }
    }
}

impl From<&str> for RoleKind {
    fn from(s: &str) -> Self {
        match s {
            "user" => Self::User,
            "admin" => Self::Admin,
            _ => Self::Custom(s.to_owned()),
        }
    }
}

impl Serialize for RoleKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RoleKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(Self::from(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_kind_serde_roundtrip() {
        for variant in [
            ConversationKind::Direct,
            ConversationKind::Group,
            ConversationKind::Inbox,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: ConversationKind = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn conversation_user_role_serde_roundtrip() {
        for variant in [
            ConversationUserRole::Owner,
            ConversationUserRole::Admin,
            ConversationUserRole::Member,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: ConversationUserRole = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn agent_mode_serde_roundtrip() {
        for variant in [AgentMode::Always, AgentMode::Mention, AgentMode::Silent] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: AgentMode = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn agent_mode_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&AgentMode::Always).unwrap(),
            "\"always\""
        );
        assert_eq!(
            serde_json::to_string(&AgentMode::Mention).unwrap(),
            "\"mention\""
        );
        assert_eq!(
            serde_json::to_string(&AgentMode::Silent).unwrap(),
            "\"silent\""
        );
    }

    #[test]
    fn scope_kind_serde_roundtrip() {
        for variant in [
            ScopeKind::System,
            ScopeKind::User,
            ScopeKind::Group,
            ScopeKind::Session,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: ScopeKind = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn scope_kind_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&ScopeKind::System).unwrap(),
            "\"system\""
        );
        assert_eq!(serde_json::to_string(&ScopeKind::User).unwrap(), "\"user\"");
    }

    #[test]
    fn user_status_serde_roundtrip() {
        for variant in [
            UserStatus::Pending,
            UserStatus::Active,
            UserStatus::Disabled,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: UserStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn message_role_serde_roundtrip() {
        for variant in [
            MessageRole::User,
            MessageRole::Assistant,
            MessageRole::System,
            MessageRole::Event,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: MessageRole = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn message_role_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&MessageRole::Assistant).unwrap(),
            "\"assistant\""
        );
        assert_eq!(
            serde_json::to_string(&MessageRole::Event).unwrap(),
            "\"event\""
        );
    }

    #[test]
    fn tool_execution_source_serde_roundtrip() {
        for variant in [
            ToolExecutionSource::Builtin,
            ToolExecutionSource::Plugin,
            ToolExecutionSource::Mcp,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: ToolExecutionSource = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn tool_execution_status_serde_roundtrip() {
        for variant in [
            ToolExecutionStatus::Pending,
            ToolExecutionStatus::Running,
            ToolExecutionStatus::Completed,
            ToolExecutionStatus::Failed,
            ToolExecutionStatus::Cancelled,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: ToolExecutionStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn job_status_serde_roundtrip() {
        for variant in [JobStatus::Active, JobStatus::Paused, JobStatus::Cancelled] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: JobStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn permission_mode_serde_roundtrip() {
        for variant in [
            PermissionMode::Interactive,
            PermissionMode::PolicyBased,
            PermissionMode::Autonomous,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: PermissionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn permission_mode_default_is_policy_based() {
        assert_eq!(PermissionMode::default(), PermissionMode::PolicyBased);
    }

    #[test]
    fn sandbox_net_mode_serde_roundtrip() {
        for variant in [
            SandboxNetMode::None,
            SandboxNetMode::AllowedDomains,
            SandboxNetMode::Full,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: SandboxNetMode = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn workspace_state_serde_roundtrip() {
        for variant in [
            WorkspaceState::Active,
            WorkspaceState::Archived,
            WorkspaceState::Deleted,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: WorkspaceState = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn worktree_state_serde_roundtrip() {
        for variant in [
            WorktreeState::Active,
            WorktreeState::Stale,
            WorktreeState::Removing,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: WorktreeState = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn artifact_state_serde_roundtrip() {
        for variant in [
            ArtifactState::Draft,
            ArtifactState::Proposed,
            ArtifactState::Approved,
            ArtifactState::Rejected,
            ArtifactState::Archived,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: ArtifactState = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn artifact_relation_serde_roundtrip() {
        for variant in [
            ArtifactRelation::SpawnedBy,
            ArtifactRelation::Supersedes,
            ArtifactRelation::References,
            ArtifactRelation::Implements,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: ArtifactRelation = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn artifact_kind_serde_roundtrip() {
        for variant in [
            ArtifactKind::CodeChange,
            ArtifactKind::Document,
            ArtifactKind::Proposal,
            ArtifactKind::Snapshot,
            ArtifactKind::Trace,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: ArtifactKind = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn role_kind_from_known_strings() {
        assert_eq!(RoleKind::from("user"), RoleKind::User);
        assert_eq!(RoleKind::from("admin"), RoleKind::Admin);
    }

    #[test]
    fn role_kind_from_custom_string() {
        let role = RoleKind::from("moderator");
        assert_eq!(role, RoleKind::Custom("moderator".into()));
        assert_eq!(role.as_str(), "moderator");
    }

    #[test]
    fn role_kind_serde_roundtrip() {
        for variant in [
            RoleKind::User,
            RoleKind::Admin,
            RoleKind::Custom("editor".into()),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: RoleKind = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn role_kind_serializes_as_string() {
        assert_eq!(serde_json::to_string(&RoleKind::User).unwrap(), "\"user\"");
        assert_eq!(
            serde_json::to_string(&RoleKind::Admin).unwrap(),
            "\"admin\""
        );
        assert_eq!(
            serde_json::to_string(&RoleKind::Custom("mod".into())).unwrap(),
            "\"mod\""
        );
    }

    #[test]
    fn role_kind_display() {
        assert_eq!(RoleKind::User.to_string(), "user");
        assert_eq!(RoleKind::Admin.to_string(), "admin");
        assert_eq!(RoleKind::Custom("custom".into()).to_string(), "custom");
    }
}
