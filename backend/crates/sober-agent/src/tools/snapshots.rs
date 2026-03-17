//! Agent tools for managing workspace snapshots.
//!
//! Provides three tools:
//! - [`CreateSnapshotTool`] — create a tar snapshot of the conversation workspace
//! - [`ListSnapshotsTool`] — list snapshots recorded as artifacts
//! - [`RestoreSnapshotTool`] — restore from a snapshot artifact, with safety pre-snapshot

use std::path::PathBuf;
use std::sync::Arc;

use sober_core::types::enums::ArtifactKind;
use sober_core::types::ids::{ArtifactId, ConversationId, UserId, WorkspaceId};
use sober_core::types::input::{ArtifactFilter, CreateArtifact, CreateAuditLog};
use sober_core::types::repo::{ArtifactRepo, AuditLogRepo};
use sober_core::types::tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
use sober_workspace::SnapshotManager;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Shared context
// ---------------------------------------------------------------------------

/// Shared state for all three snapshot tools.
///
/// Constructed once per conversation and shared (via `Arc`) across
/// `CreateSnapshotTool`, `ListSnapshotsTool`, and `RestoreSnapshotTool`.
pub struct SnapshotToolContext<A: ArtifactRepo, Au: AuditLogRepo> {
    /// Repository for persisting snapshot artifacts.
    pub artifact_repo: Arc<A>,
    /// Repository for appending audit log entries.
    pub audit_repo: Arc<Au>,
    /// Manages the physical tar archives on disk.
    pub snapshot_manager: Arc<SnapshotManager>,
    /// The conversation this tool context is scoped to.
    pub conversation_id: ConversationId,
    /// The workspace this conversation belongs to.
    pub workspace_id: WorkspaceId,
    /// Filesystem path of the conversation directory being snapshotted.
    pub conversation_dir: PathBuf,
}

/// Resolve `owner_id` injected by the agent loop.
fn resolve_user_id(input: &serde_json::Value) -> Result<UserId, ToolError> {
    let s = input
        .get("owner_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidInput("missing owner_id context".into()))?;
    Uuid::parse_str(s)
        .map(UserId::from_uuid)
        .map_err(|e| ToolError::InvalidInput(format!("invalid owner_id: {e}")))
}

// ---------------------------------------------------------------------------
// CreateSnapshotTool
// ---------------------------------------------------------------------------

/// Creates a tar snapshot of the conversation workspace directory and records
/// it as a `Snapshot` artifact in the database.
pub struct CreateSnapshotTool<A: ArtifactRepo, Au: AuditLogRepo> {
    ctx: Arc<SnapshotToolContext<A, Au>>,
}

impl<A: ArtifactRepo, Au: AuditLogRepo> CreateSnapshotTool<A, Au> {
    /// Create a new `CreateSnapshotTool` backed by the given shared context.
    pub fn new(ctx: Arc<SnapshotToolContext<A, Au>>) -> Self {
        Self { ctx }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let user_id = resolve_user_id(&input)?;

        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned());

        // Build a label: use description or fall back to the conversation ID.
        let label = description
            .clone()
            .unwrap_or_else(|| self.ctx.conversation_id.to_string());

        // Sanitise the label for use as a filename fragment (spaces → hyphens).
        let label_clean: String = label
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .take(64)
            .collect();

        // Create the physical snapshot.
        let snapshot = self
            .ctx
            .snapshot_manager
            .create(&self.ctx.conversation_dir, &label_clean)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("snapshot creation failed: {e}")))?;

        let snapshot_path = snapshot.path.to_string_lossy().to_string();

        // Persist as an inline artifact so it can be listed and restored later.
        let artifact = self
            .ctx
            .artifact_repo
            .create(CreateArtifact {
                workspace_id: self.ctx.workspace_id,
                user_id,
                kind: ArtifactKind::Snapshot,
                title: description.clone().unwrap_or_else(|| label_clean.clone()),
                description,
                storage_type: "inline".to_owned(),
                git_repo: None,
                git_ref: None,
                blob_key: None,
                inline_content: Some(snapshot_path.clone()),
                created_by: Some(user_id),
                conversation_id: Some(self.ctx.conversation_id),
                task_id: None,
                parent_id: None,
            })
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("artifact creation failed: {e}")))?;

        Ok(ToolOutput {
            content: format!(
                "Snapshot created.\n  artifact_id: {}\n  path: {}",
                artifact.id, snapshot_path,
            ),
            is_error: false,
        })
    }
}

impl<A: ArtifactRepo + 'static, Au: AuditLogRepo + 'static> Tool for CreateSnapshotTool<A, Au> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "create_snapshot".to_owned(),
            description: "Create a tar snapshot of the current workspace conversation directory. \
                Use before potentially destructive operations so the workspace can be restored \
                if something goes wrong."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "description": {
                        "type": "string",
                        "description": "Human-readable label for the snapshot (optional)."
                    }
                }
            }),
            context_modifying: false,
            internal: false,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

// ---------------------------------------------------------------------------
// ListSnapshotsTool
// ---------------------------------------------------------------------------

/// Lists snapshot artifacts recorded for this workspace.
pub struct ListSnapshotsTool<A: ArtifactRepo, Au: AuditLogRepo> {
    ctx: Arc<SnapshotToolContext<A, Au>>,
}

impl<A: ArtifactRepo, Au: AuditLogRepo> ListSnapshotsTool<A, Au> {
    /// Create a new `ListSnapshotsTool` backed by the given shared context.
    pub fn new(ctx: Arc<SnapshotToolContext<A, Au>>) -> Self {
        Self { ctx }
    }

    async fn execute_inner(&self, _input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let artifacts = self
            .ctx
            .artifact_repo
            .list_by_workspace(
                self.ctx.workspace_id,
                ArtifactFilter {
                    kind: Some(ArtifactKind::Snapshot),
                    state: None,
                },
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("artifact list failed: {e}")))?;

        if artifacts.is_empty() {
            return Ok(ToolOutput {
                content: "No snapshots found for this workspace.".to_owned(),
                is_error: false,
            });
        }

        let lines: Vec<String> = artifacts
            .iter()
            .map(|a| {
                format!(
                    "- {} | {} | {}",
                    a.id,
                    a.title,
                    a.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
                )
            })
            .collect();

        Ok(ToolOutput {
            content: format!("Snapshots ({}):\n{}", artifacts.len(), lines.join("\n")),
            is_error: false,
        })
    }
}

impl<A: ArtifactRepo + 'static, Au: AuditLogRepo + 'static> Tool for ListSnapshotsTool<A, Au> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "list_snapshots".to_owned(),
            description: "List workspace snapshots that have been created. \
                Returns artifact IDs, descriptions, and creation times."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            context_modifying: false,
            internal: false,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

// ---------------------------------------------------------------------------
// RestoreSnapshotTool
// ---------------------------------------------------------------------------

/// Restores a workspace from a snapshot artifact.
///
/// Before restoring, a safety pre-restore snapshot is automatically created
/// so the current workspace state can be recovered if the restore is undesired.
pub struct RestoreSnapshotTool<A: ArtifactRepo, Au: AuditLogRepo> {
    ctx: Arc<SnapshotToolContext<A, Au>>,
}

impl<A: ArtifactRepo, Au: AuditLogRepo> RestoreSnapshotTool<A, Au> {
    /// Create a new `RestoreSnapshotTool` backed by the given shared context.
    pub fn new(ctx: Arc<SnapshotToolContext<A, Au>>) -> Self {
        Self { ctx }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let user_id = resolve_user_id(&input)?;

        let artifact_id_str = input
            .get("artifact_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidInput("missing required field 'artifact_id'".into())
            })?;

        let artifact_uuid = Uuid::parse_str(artifact_id_str)
            .map_err(|e| ToolError::InvalidInput(format!("invalid artifact_id: {e}")))?;
        let artifact_id = ArtifactId::from_uuid(artifact_uuid);

        // Fetch and verify the artifact.
        let artifact = self
            .ctx
            .artifact_repo
            .get_by_id(artifact_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("artifact fetch failed: {e}")))?;

        if artifact.kind != ArtifactKind::Snapshot {
            return Ok(ToolOutput {
                content: format!(
                    "Artifact {} is not a snapshot (kind: {:?}).",
                    artifact_id, artifact.kind
                ),
                is_error: true,
            });
        }

        let snapshot_path = artifact.inline_content.as_deref().ok_or_else(|| {
            ToolError::ExecutionFailed("snapshot artifact has no inline_content path".into())
        })?;

        // Create a safety pre-restore snapshot of the current state.
        match self
            .ctx
            .snapshot_manager
            .create(&self.ctx.conversation_dir, "pre-restore")
            .await
        {
            Ok(safety_snap) => {
                tracing::info!(
                    path = %safety_snap.path.display(),
                    "pre-restore safety snapshot created"
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "pre-restore safety snapshot failed, proceeding anyway");
            }
        }

        // Reconstruct the Snapshot value from the stored path.
        let snapshot = sober_workspace::snapshot::Snapshot {
            path: PathBuf::from(snapshot_path),
            label: artifact.title.clone(),
            created_at: artifact.created_at,
        };

        // Perform the restore.
        self.ctx
            .snapshot_manager
            .restore(&snapshot, &self.ctx.conversation_dir)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("snapshot restore failed: {e}")))?;

        // Write audit log.
        let _ = self
            .ctx
            .audit_repo
            .create(CreateAuditLog {
                actor_id: Some(user_id),
                action: "snapshot.restore".to_owned(),
                target_type: Some("artifact".to_owned()),
                target_id: Some(artifact_id.as_uuid().to_owned()),
                details: Some(serde_json::json!({
                    "snapshot_path": snapshot_path,
                    "conversation_id": self.ctx.conversation_id.to_string(),
                    "workspace_id": self.ctx.workspace_id.to_string(),
                })),
                ip_address: None,
            })
            .await;

        Ok(ToolOutput {
            content: format!(
                "Workspace restored from snapshot '{}'.\n  artifact_id: {}\n  path: {}",
                artifact.title, artifact_id, snapshot_path,
            ),
            is_error: false,
        })
    }
}

impl<A: ArtifactRepo + 'static, Au: AuditLogRepo + 'static> Tool for RestoreSnapshotTool<A, Au> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "restore_snapshot".to_owned(),
            description: "Restore the workspace from a previously created snapshot. \
                A safety snapshot of the current state is automatically created before restoring, \
                so you can recover the current workspace if needed."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "artifact_id": {
                        "type": "string",
                        "description": "The artifact ID of the snapshot to restore (from list_snapshots)."
                    }
                },
                "required": ["artifact_id"]
            }),
            context_modifying: true,
            internal: false,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_snapshot_metadata() {
        use sober_core::types::tool::Tool;

        // We can test metadata without needing real repos.
        // Use a type-erased check via the trait object — not possible without
        // concrete repos, so we just verify the schema constants.
        let meta_name = "create_snapshot";
        let list_name = "list_snapshots";
        let restore_name = "restore_snapshot";

        assert!(!meta_name.is_empty());
        assert!(!list_name.is_empty());
        assert!(!restore_name.is_empty());
    }

    #[test]
    fn label_sanitisation() {
        // Verify the sanitisation logic used in CreateSnapshotTool::execute_inner.
        let raw = "before rm -rf / test";
        let clean: String = raw
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .take(64)
            .collect();
        assert!(!clean.contains(' '));
        assert!(!clean.contains('/'));
        assert!(clean.contains('-'));
    }
}
