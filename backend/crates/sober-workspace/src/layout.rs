//! Workspace filesystem layout helpers.

use std::path::{Path, PathBuf};

use sober_core::types::ConversationId;

use crate::error::WorkspaceError;

/// Returns the conversation-specific directory under a workspace root.
/// Creates the directory if it does not exist.
pub async fn ensure_conversation_dir(
    workspace_root: &Path,
    conversation_id: ConversationId,
) -> Result<PathBuf, WorkspaceError> {
    let dir = workspace_root.join(conversation_id.to_string());
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(WorkspaceError::Filesystem)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn creates_conversation_dir() {
        let tmp = TempDir::new().unwrap();
        let conv_id = ConversationId::new();
        let path = ensure_conversation_dir(tmp.path(), conv_id).await.unwrap();
        assert!(path.exists());
        assert_eq!(path, tmp.path().join(conv_id.to_string()));
    }

    #[tokio::test]
    async fn idempotent_on_existing_dir() {
        let tmp = TempDir::new().unwrap();
        let conv_id = ConversationId::new();
        let p1 = ensure_conversation_dir(tmp.path(), conv_id).await.unwrap();
        let p2 = ensure_conversation_dir(tmp.path(), conv_id).await.unwrap();
        assert_eq!(p1, p2);
    }
}
