//! Workspace snapshot creation and restoration.
//!
//! Snapshots are tar archives of the workspace root directory. They provide
//! a simple rollback mechanism before potentially destructive operations.

use std::path::{Path, PathBuf};

use chrono::Utc;
use tokio::fs;
use tokio::process::Command;

use crate::WorkspaceError;

/// Metadata for a created snapshot.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Path to the tar archive.
    pub path: PathBuf,
    /// Human-readable label.
    pub label: String,
    /// When the snapshot was created.
    pub created_at: chrono::DateTime<Utc>,
}

/// Manages workspace snapshots (tar archives).
pub struct SnapshotManager {
    snapshot_dir: PathBuf,
}

impl SnapshotManager {
    /// Create a new snapshot manager with the given storage directory.
    pub fn new(snapshot_dir: PathBuf) -> Self {
        Self { snapshot_dir }
    }

    /// Create a tar snapshot of the workspace root.
    pub async fn create(
        &self,
        workspace_root: &Path,
        label: &str,
    ) -> Result<Snapshot, WorkspaceError> {
        fs::create_dir_all(&self.snapshot_dir)
            .await
            .map_err(WorkspaceError::Filesystem)?;

        let now = Utc::now();
        let filename = format!("{}-{}.tar", now.format("%Y%m%d%H%M%S%3f"), label);
        let snap_path = self.snapshot_dir.join(&filename);

        let output = Command::new("tar")
            .arg("cf")
            .arg(&snap_path)
            .arg("-C")
            .arg(workspace_root)
            .arg(".")
            .output()
            .await
            .map_err(|e| WorkspaceError::Snapshot(format!("tar failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(WorkspaceError::Snapshot(format!(
                "tar exited {}: {stderr}",
                output.status
            )));
        }

        Ok(Snapshot {
            path: snap_path,
            label: label.to_string(),
            created_at: now,
        })
    }

    /// Restore a snapshot by extracting it over the workspace root.
    pub async fn restore(
        &self,
        snapshot: &Snapshot,
        workspace_root: &Path,
    ) -> Result<(), WorkspaceError> {
        let output = Command::new("tar")
            .arg("xf")
            .arg(&snapshot.path)
            .arg("-C")
            .arg(workspace_root)
            .output()
            .await
            .map_err(|e| WorkspaceError::Snapshot(format!("tar restore failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(WorkspaceError::Snapshot(format!(
                "tar restore exited {}: {stderr}",
                output.status
            )));
        }

        Ok(())
    }

    /// List all snapshots in the snapshot directory, sorted oldest first.
    pub async fn list(&self) -> Result<Vec<Snapshot>, WorkspaceError> {
        let mut entries = fs::read_dir(&self.snapshot_dir)
            .await
            .map_err(WorkspaceError::Filesystem)?;

        let mut snapshots = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(WorkspaceError::Filesystem)?
        {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "tar") {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                // Filename format: "YYYYMMDDHHMMSS###-label"
                // The timestamp+millis is 18 chars, then '-', then label
                let label = if name.len() > 18 {
                    name[19..].to_string()
                } else {
                    name.to_string()
                };
                snapshots.push(Snapshot {
                    path,
                    label,
                    created_at: Utc::now(), // approximate; could parse from filename
                });
            }
        }

        // Sort by filename (which starts with timestamp) — oldest first
        snapshots.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(snapshots)
    }

    /// Prune oldest snapshots exceeding `max_snapshots`.
    ///
    /// Returns the number of snapshots removed.
    pub async fn prune(&self, max_snapshots: u32) -> Result<u32, WorkspaceError> {
        let snapshots = self.list().await?;
        let to_remove = snapshots.len().saturating_sub(max_snapshots as usize);
        let mut removed = 0u32;

        for snap in snapshots.iter().take(to_remove) {
            fs::remove_file(&snap.path)
                .await
                .map_err(WorkspaceError::Filesystem)?;
            removed += 1;
        }

        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn create_snapshot_produces_tar() {
        let tmp = TempDir::new().unwrap();
        let ws_root = tmp.path().join("workspace");
        tokio::fs::create_dir_all(&ws_root).await.unwrap();
        tokio::fs::write(ws_root.join("file.txt"), b"content")
            .await
            .unwrap();

        let snap_dir = tmp.path().join("snapshots");
        let mgr = SnapshotManager::new(snap_dir);
        let snap = mgr.create(&ws_root, "pre-shell").await.unwrap();

        assert!(snap.path.exists());
        assert!(snap.path.extension().is_some_and(|e| e == "tar"));
        assert_eq!(snap.label, "pre-shell");
    }

    #[tokio::test]
    async fn restore_snapshot_overwrites_workspace() {
        let tmp = TempDir::new().unwrap();
        let ws_root = tmp.path().join("workspace");
        tokio::fs::create_dir_all(&ws_root).await.unwrap();
        tokio::fs::write(ws_root.join("file.txt"), b"original")
            .await
            .unwrap();

        let snap_dir = tmp.path().join("snapshots");
        let mgr = SnapshotManager::new(snap_dir);
        let snap = mgr.create(&ws_root, "backup").await.unwrap();

        // Modify workspace
        tokio::fs::write(ws_root.join("file.txt"), b"modified")
            .await
            .unwrap();

        mgr.restore(&snap, &ws_root).await.unwrap();
        let content = tokio::fs::read_to_string(ws_root.join("file.txt"))
            .await
            .unwrap();
        assert_eq!(content, "original");
    }

    #[tokio::test]
    async fn list_snapshots() {
        let tmp = TempDir::new().unwrap();
        let ws_root = tmp.path().join("workspace");
        tokio::fs::create_dir_all(&ws_root).await.unwrap();

        let snap_dir = tmp.path().join("snapshots");
        let mgr = SnapshotManager::new(snap_dir);
        mgr.create(&ws_root, "snap-1").await.unwrap();
        // Small delay so filenames differ
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        mgr.create(&ws_root, "snap-2").await.unwrap();

        let snaps = mgr.list().await.unwrap();
        assert_eq!(snaps.len(), 2);
    }

    #[tokio::test]
    async fn prune_removes_oldest_snapshots() {
        let tmp = TempDir::new().unwrap();
        let ws_root = tmp.path().join("workspace");
        tokio::fs::create_dir_all(&ws_root).await.unwrap();
        tokio::fs::write(ws_root.join("file.txt"), b"data")
            .await
            .unwrap();

        let snap_dir = tmp.path().join("snapshots");
        let mgr = SnapshotManager::new(snap_dir);

        for i in 0..4 {
            mgr.create(&ws_root, &format!("snap-{i}")).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        assert_eq!(mgr.list().await.unwrap().len(), 4);

        let removed = mgr.prune(2).await.unwrap();
        assert_eq!(removed, 2);
        assert_eq!(mgr.list().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn prune_noop_when_under_limit() {
        let tmp = TempDir::new().unwrap();
        let ws_root = tmp.path().join("workspace");
        tokio::fs::create_dir_all(&ws_root).await.unwrap();

        let snap_dir = tmp.path().join("snapshots");
        let mgr = SnapshotManager::new(snap_dir);
        mgr.create(&ws_root, "only-one").await.unwrap();

        let removed = mgr.prune(10).await.unwrap();
        assert_eq!(removed, 0);
        assert_eq!(mgr.list().await.unwrap().len(), 1);
    }
}
