//! Agent tools for workspace artifact management.
//!
//! Provides four tools for the agent to create, list, read, and delete
//! (archive) artifacts within a workspace:
//!
//! - [`CreateArtifactTool`] — create a new artifact (inline or blob storage)
//! - [`ListArtifactsTool`] — list artifacts with optional kind/state filters
//! - [`ReadArtifactTool`] — read an artifact's content by ID
//! - [`DeleteArtifactTool`] — archive an artifact by ID

use std::sync::Arc;

use sober_core::types::enums::{ArtifactKind, ArtifactState};
use sober_core::types::input::{ArtifactFilter, CreateArtifact};
use sober_core::types::repo::{ArtifactRepo, AuditLogRepo};
use sober_core::types::tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
use sober_core::{ArtifactId, ConversationId, UserId, WorkspaceId};
use sober_workspace::BlobStore;
use uuid::Uuid;

/// Shared context for all artifact tools.
pub struct ArtifactToolContext<A: ArtifactRepo, Au: AuditLogRepo> {
    /// Repository for artifact CRUD operations.
    pub artifact_repo: Arc<A>,
    /// Repository for audit logging.
    pub audit_repo: Arc<Au>,
    /// Content-addressed blob store for blob-backed artifacts.
    pub blob_store: Arc<BlobStore>,
    /// The current user's ID.
    pub user_id: UserId,
    /// The conversation this tool invocation is associated with.
    pub conversation_id: ConversationId,
    /// The workspace these artifacts belong to.
    pub workspace_id: WorkspaceId,
}

/// Parses a string into an [`ArtifactKind`].
fn parse_artifact_kind(s: &str) -> Result<ArtifactKind, ToolError> {
    match s {
        "code_change" => Ok(ArtifactKind::CodeChange),
        "document" => Ok(ArtifactKind::Document),
        "proposal" => Ok(ArtifactKind::Proposal),
        "snapshot" => Ok(ArtifactKind::Snapshot),
        "trace" => Ok(ArtifactKind::Trace),
        other => Err(ToolError::InvalidInput(format!(
            "unknown kind '{other}'. Use: code_change, document, proposal, snapshot, trace"
        ))),
    }
}

/// Parses a string into an [`ArtifactState`].
fn parse_artifact_state(s: &str) -> Result<ArtifactState, ToolError> {
    match s {
        "draft" => Ok(ArtifactState::Draft),
        "proposed" => Ok(ArtifactState::Proposed),
        "approved" => Ok(ArtifactState::Approved),
        "rejected" => Ok(ArtifactState::Rejected),
        "archived" => Ok(ArtifactState::Archived),
        other => Err(ToolError::InvalidInput(format!(
            "unknown state '{other}'. Use: draft, proposed, approved, rejected, archived"
        ))),
    }
}

/// Returns a human-readable label for an [`ArtifactKind`].
fn kind_label(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::CodeChange => "code_change",
        ArtifactKind::Document => "document",
        ArtifactKind::Proposal => "proposal",
        ArtifactKind::Snapshot => "snapshot",
        ArtifactKind::Trace => "trace",
    }
}

/// Returns a human-readable label for an [`ArtifactState`].
fn state_label(state: ArtifactState) -> &'static str {
    match state {
        ArtifactState::Draft => "draft",
        ArtifactState::Proposed => "proposed",
        ArtifactState::Approved => "approved",
        ArtifactState::Rejected => "rejected",
        ArtifactState::Archived => "archived",
    }
}

// ---------------------------------------------------------------------------
// CreateArtifactTool
// ---------------------------------------------------------------------------

/// Tool for creating workspace artifacts (documents, proposals, code changes, etc.).
pub struct CreateArtifactTool<A: ArtifactRepo, Au: AuditLogRepo> {
    ctx: Arc<ArtifactToolContext<A, Au>>,
}

impl<A: ArtifactRepo, Au: AuditLogRepo> CreateArtifactTool<A, Au> {
    /// Creates a new create-artifact tool.
    pub fn new(ctx: Arc<ArtifactToolContext<A, Au>>) -> Self {
        Self { ctx }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let title = input
            .get("title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'title'".into()))?
            .to_owned();

        let kind_str = input
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'kind'".into()))?;
        let kind = parse_artifact_kind(kind_str)?;

        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'content'".into()))?
            .to_owned();

        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);

        let storage_type = input
            .get("storage_type")
            .and_then(|v| v.as_str())
            .unwrap_or("inline");

        let (inline_content, blob_key, storage_type_str) = match storage_type {
            "blob" => {
                let key = self
                    .ctx
                    .blob_store
                    .store(content.as_bytes())
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(format!("blob storage failed: {e}")))?;
                (None, Some(key), "blob".to_owned())
            }
            _ => (Some(content.clone()), None, "inline".to_owned()),
        };

        let create_input = CreateArtifact {
            workspace_id: self.ctx.workspace_id,
            user_id: self.ctx.user_id,
            kind,
            title: title.clone(),
            description,
            storage_type: storage_type_str,
            blob_key,
            inline_content,
            created_by: None, // None = agent-created
            conversation_id: Some(self.ctx.conversation_id),
            ..Default::default()
        };

        let artifact = self
            .ctx
            .artifact_repo
            .create(create_input)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to create artifact: {e}")))?;

        Ok(ToolOutput {
            content: format!(
                "Created {} artifact '{}' (id: {}, state: {})",
                kind_label(artifact.kind),
                artifact.title,
                artifact.id,
                state_label(artifact.state),
            ),
            is_error: false,
        })
    }
}

impl<A: ArtifactRepo + 'static, Au: AuditLogRepo + 'static> Tool for CreateArtifactTool<A, Au> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "create_artifact".to_owned(),
            description: "Create a workspace artifact such as a document, proposal, code change, \
                snapshot, or trace. Content can be stored inline or as a blob."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Display title for the artifact."
                    },
                    "description": {
                        "type": "string",
                        "description": "Optional description of the artifact."
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["code_change", "document", "proposal", "snapshot", "trace"],
                        "description": "The type of artifact."
                    },
                    "storage_type": {
                        "type": "string",
                        "enum": ["inline", "blob"],
                        "default": "inline",
                        "description": "How to store the content: 'inline' in the database or 'blob' in content-addressed storage."
                    },
                    "content": {
                        "type": "string",
                        "description": "The artifact content."
                    }
                },
                "required": ["title", "kind", "content"]
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
// ListArtifactsTool
// ---------------------------------------------------------------------------

/// Tool for listing artifacts in the current workspace.
pub struct ListArtifactsTool<A: ArtifactRepo, Au: AuditLogRepo> {
    ctx: Arc<ArtifactToolContext<A, Au>>,
}

impl<A: ArtifactRepo, Au: AuditLogRepo> ListArtifactsTool<A, Au> {
    /// Creates a new list-artifacts tool.
    pub fn new(ctx: Arc<ArtifactToolContext<A, Au>>) -> Self {
        Self { ctx }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let kind = input
            .get("kind")
            .and_then(|v| v.as_str())
            .map(parse_artifact_kind)
            .transpose()?;

        let state = input
            .get("state")
            .and_then(|v| v.as_str())
            .map(parse_artifact_state)
            .transpose()?;

        let filter = ArtifactFilter { kind, state };

        let artifacts = self
            .ctx
            .artifact_repo
            .list_by_workspace(self.ctx.workspace_id, filter)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to list artifacts: {e}")))?;

        if artifacts.is_empty() {
            return Ok(ToolOutput {
                content: "No artifacts found matching the given filters.".to_owned(),
                is_error: false,
            });
        }

        let mut output = format!("Found {} artifacts:\n\n", artifacts.len());
        for (i, artifact) in artifacts.iter().enumerate() {
            output.push_str(&format!(
                "{}. [{}] {} — {} (id: {}, created: {})\n",
                i + 1,
                kind_label(artifact.kind),
                artifact.title,
                state_label(artifact.state),
                artifact.id,
                artifact.created_at.format("%Y-%m-%d %H:%M"),
            ));
            if let Some(desc) = &artifact.description {
                output.push_str(&format!("   {desc}\n"));
            }
        }

        Ok(ToolOutput {
            content: output,
            is_error: false,
        })
    }
}

impl<A: ArtifactRepo + 'static, Au: AuditLogRepo + 'static> Tool for ListArtifactsTool<A, Au> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "list_artifacts".to_owned(),
            description: "List artifacts in the current workspace. Optionally filter by kind \
                and/or state."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["code_change", "document", "proposal", "snapshot", "trace"],
                        "description": "Filter by artifact kind (optional)."
                    },
                    "state": {
                        "type": "string",
                        "enum": ["draft", "proposed", "approved", "rejected", "archived"],
                        "description": "Filter by artifact state (optional)."
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
// ReadArtifactTool
// ---------------------------------------------------------------------------

/// Tool for reading an artifact's content by ID.
pub struct ReadArtifactTool<A: ArtifactRepo, Au: AuditLogRepo> {
    ctx: Arc<ArtifactToolContext<A, Au>>,
}

impl<A: ArtifactRepo, Au: AuditLogRepo> ReadArtifactTool<A, Au> {
    /// Creates a new read-artifact tool.
    pub fn new(ctx: Arc<ArtifactToolContext<A, Au>>) -> Self {
        Self { ctx }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let id_str = input
            .get("artifact_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidInput("missing required field 'artifact_id'".into())
            })?;

        let uuid = Uuid::parse_str(id_str)
            .map_err(|e| ToolError::InvalidInput(format!("invalid artifact_id: {e}")))?;
        let artifact_id = ArtifactId::from_uuid(uuid);

        let artifact = self
            .ctx
            .artifact_repo
            .get_by_id(artifact_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to get artifact: {e}")))?;

        let content = match artifact.storage_type.as_str() {
            "blob" => {
                let key = artifact.blob_key.as_deref().ok_or_else(|| {
                    ToolError::ExecutionFailed("blob artifact missing blob_key".into())
                })?;
                let data =
                    self.ctx.blob_store.retrieve(key).await.map_err(|e| {
                        ToolError::ExecutionFailed(format!("blob read failed: {e}"))
                    })?;
                String::from_utf8(data).map_err(|e| {
                    ToolError::ExecutionFailed(format!("blob content is not valid UTF-8: {e}"))
                })?
            }
            "inline" => artifact.inline_content.clone().unwrap_or_default(),
            other => {
                return Err(ToolError::ExecutionFailed(format!(
                    "unsupported storage type '{other}' — only 'inline' and 'blob' are readable"
                )));
            }
        };

        let mut output = format!(
            "Artifact: {} ({})\nKind: {} | State: {} | Storage: {}\n",
            artifact.title,
            artifact.id,
            kind_label(artifact.kind),
            state_label(artifact.state),
            artifact.storage_type,
        );
        if let Some(desc) = &artifact.description {
            output.push_str(&format!("Description: {desc}\n"));
        }
        output.push_str(&format!(
            "Created: {}\n\n",
            artifact.created_at.format("%Y-%m-%d %H:%M")
        ));
        output.push_str(&content);

        Ok(ToolOutput {
            content: output,
            is_error: false,
        })
    }
}

impl<A: ArtifactRepo + 'static, Au: AuditLogRepo + 'static> Tool for ReadArtifactTool<A, Au> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "read_artifact".to_owned(),
            description: "Read the full content of a workspace artifact by its ID.".to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "artifact_id": {
                        "type": "string",
                        "description": "UUID of the artifact to read."
                    }
                },
                "required": ["artifact_id"]
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
// DeleteArtifactTool
// ---------------------------------------------------------------------------

/// Tool for archiving (soft-deleting) an artifact by ID.
pub struct DeleteArtifactTool<A: ArtifactRepo, Au: AuditLogRepo> {
    ctx: Arc<ArtifactToolContext<A, Au>>,
}

impl<A: ArtifactRepo, Au: AuditLogRepo> DeleteArtifactTool<A, Au> {
    /// Creates a new delete-artifact tool.
    pub fn new(ctx: Arc<ArtifactToolContext<A, Au>>) -> Self {
        Self { ctx }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let id_str = input
            .get("artifact_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidInput("missing required field 'artifact_id'".into())
            })?;

        let uuid = Uuid::parse_str(id_str)
            .map_err(|e| ToolError::InvalidInput(format!("invalid artifact_id: {e}")))?;
        let artifact_id = ArtifactId::from_uuid(uuid);

        self.ctx
            .artifact_repo
            .update_state(artifact_id, ArtifactState::Archived)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to archive artifact: {e}")))?;

        Ok(ToolOutput {
            content: format!("Artifact {artifact_id} has been archived."),
            is_error: false,
        })
    }
}

impl<A: ArtifactRepo + 'static, Au: AuditLogRepo + 'static> Tool for DeleteArtifactTool<A, Au> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "delete_artifact".to_owned(),
            description: "Archive (soft-delete) a workspace artifact by its ID. \
                The artifact is not permanently deleted but moved to the 'archived' state."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "artifact_id": {
                        "type": "string",
                        "description": "UUID of the artifact to archive."
                    }
                },
                "required": ["artifact_id"]
            }),
            context_modifying: false,
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
    fn parse_artifact_kind_valid() {
        assert_eq!(
            parse_artifact_kind("code_change").unwrap(),
            ArtifactKind::CodeChange
        );
        assert_eq!(
            parse_artifact_kind("document").unwrap(),
            ArtifactKind::Document
        );
        assert_eq!(
            parse_artifact_kind("proposal").unwrap(),
            ArtifactKind::Proposal
        );
        assert_eq!(
            parse_artifact_kind("snapshot").unwrap(),
            ArtifactKind::Snapshot
        );
        assert_eq!(parse_artifact_kind("trace").unwrap(), ArtifactKind::Trace);
    }

    #[test]
    fn parse_artifact_kind_invalid() {
        assert!(parse_artifact_kind("unknown").is_err());
        assert!(parse_artifact_kind("").is_err());
    }

    #[test]
    fn parse_artifact_state_valid() {
        assert_eq!(parse_artifact_state("draft").unwrap(), ArtifactState::Draft);
        assert_eq!(
            parse_artifact_state("proposed").unwrap(),
            ArtifactState::Proposed
        );
        assert_eq!(
            parse_artifact_state("approved").unwrap(),
            ArtifactState::Approved
        );
        assert_eq!(
            parse_artifact_state("rejected").unwrap(),
            ArtifactState::Rejected
        );
        assert_eq!(
            parse_artifact_state("archived").unwrap(),
            ArtifactState::Archived
        );
    }

    #[test]
    fn parse_artifact_state_invalid() {
        assert!(parse_artifact_state("unknown").is_err());
        assert!(parse_artifact_state("").is_err());
    }

    #[test]
    fn kind_label_values() {
        assert_eq!(kind_label(ArtifactKind::CodeChange), "code_change");
        assert_eq!(kind_label(ArtifactKind::Document), "document");
        assert_eq!(kind_label(ArtifactKind::Proposal), "proposal");
        assert_eq!(kind_label(ArtifactKind::Snapshot), "snapshot");
        assert_eq!(kind_label(ArtifactKind::Trace), "trace");
    }

    #[test]
    fn state_label_values() {
        assert_eq!(state_label(ArtifactState::Draft), "draft");
        assert_eq!(state_label(ArtifactState::Proposed), "proposed");
        assert_eq!(state_label(ArtifactState::Approved), "approved");
        assert_eq!(state_label(ArtifactState::Rejected), "rejected");
        assert_eq!(state_label(ArtifactState::Archived), "archived");
    }
}
