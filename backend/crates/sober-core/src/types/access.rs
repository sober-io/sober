//! Access control primitives: caller context and permissions.
//!
//! [`CallerContext`] is constructed at entry points (API handler, scheduler
//! tick, replica delegation) and threaded through the call chain to
//! determine what the caller is allowed to see and do.

use serde::{Deserialize, Serialize};

use super::ids::{ScopeId, ToolId, UserId, WorkspaceId};

/// What triggered the current operation.
///
/// Determines the access tier for prompt assembly and authorization checks.
/// See the access mask table in `ARCHITECTURE.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriggerKind {
    /// Triggered by a human user interaction.
    Human,
    /// Triggered by the autonomous scheduler.
    Scheduler,
    /// Triggered by a delegated replica agent.
    Replica,
    /// Triggered by an admin CLI command.
    Admin,
}

/// Authorization permission.
///
/// Stub with key variants from `ARCHITECTURE.md`. Will be expanded by
/// `sober-auth` with full RBAC/ABAC support.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Read from a knowledge/memory scope.
    ReadKnowledge(ScopeId),
    /// Write to a knowledge/memory scope.
    WriteKnowledge(ScopeId),
    /// Execute a specific tool.
    ExecuteTool(ToolId),
    /// Install a plugin.
    InstallPlugin,
    /// Spawn a replica agent.
    SpawnReplica,
    /// Delegate a task to a replica.
    DelegateTask,
    /// Manage user accounts.
    ManageUsers,
    /// Manage groups.
    ManageGroups,
    /// View audit logs.
    AuditLogs,
}

/// Describes who triggered an operation and what they are allowed to access.
///
/// Constructed at the entry point and passed through the call chain.
/// The `permissions` field holds resolved RBAC/ABAC permissions;
/// `scope_grants` lists the scopes the caller may read from or write to.
#[derive(Debug, Clone)]
pub struct CallerContext {
    /// The authenticated user, if any. `None` for system-level triggers.
    pub user_id: Option<UserId>,
    /// What initiated this operation.
    pub trigger: TriggerKind,
    /// Resolved permissions for this caller.
    pub permissions: Vec<Permission>,
    /// Scopes this caller is authorized to access.
    pub scope_grants: Vec<ScopeId>,
    /// The workspace this operation is scoped to, if any.
    pub workspace_id: Option<WorkspaceId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_kind_serde_roundtrip() {
        for variant in [
            TriggerKind::Human,
            TriggerKind::Scheduler,
            TriggerKind::Replica,
            TriggerKind::Admin,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: TriggerKind = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn caller_context_for_human() {
        let user_id = UserId::new();
        let scope = ScopeId::new();
        let ctx = CallerContext {
            user_id: Some(user_id),
            trigger: TriggerKind::Human,
            permissions: vec![Permission::ReadKnowledge(scope)],
            scope_grants: vec![scope],
            workspace_id: None,
        };
        assert_eq!(ctx.trigger, TriggerKind::Human);
        assert_eq!(ctx.user_id, Some(user_id));
        assert_eq!(ctx.scope_grants.len(), 1);
    }

    #[test]
    fn caller_context_for_scheduler() {
        let ctx = CallerContext {
            user_id: None,
            trigger: TriggerKind::Scheduler,
            permissions: vec![],
            scope_grants: vec![],
            workspace_id: None,
        };
        assert_eq!(ctx.trigger, TriggerKind::Scheduler);
        assert!(ctx.user_id.is_none());
    }

    #[test]
    fn caller_context_with_workspace() {
        let user_id = UserId::new();
        let scope = ScopeId::new();
        let workspace_id = WorkspaceId::new();
        let ctx = CallerContext {
            user_id: Some(user_id),
            trigger: TriggerKind::Human,
            permissions: vec![Permission::ReadKnowledge(scope)],
            scope_grants: vec![scope],
            workspace_id: Some(workspace_id),
        };
        assert_eq!(ctx.workspace_id, Some(workspace_id));
    }
}
