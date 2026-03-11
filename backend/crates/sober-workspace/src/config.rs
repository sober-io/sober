//! Workspace operational defaults.
//!
//! These are compile-time defaults used as fallbacks when `.sober/config.toml`
//! does not specify a value. Infrastructure config (DB URLs, ports) lives in
//! `AppConfig` — these are workspace-specific operational settings.

use std::path::PathBuf;

/// Operational defaults for the workspace system.
#[derive(Debug, Clone)]
pub struct WorkspaceDefaults {
    /// Root directory for all workspace data (blobs, snapshots).
    pub data_root: PathBuf,
    /// Blob retention period after workspace deletion (days).
    pub blob_retention_days: u32,
    /// Workspace archive grace period before hard delete (days).
    pub archive_grace_period_days: u32,
    /// Worktree stale threshold (hours of inactivity).
    pub worktree_stale_hours: u32,
    /// Maximum snapshots to keep per workspace.
    pub max_snapshots: u32,
}

impl Default for WorkspaceDefaults {
    fn default() -> Self {
        Self {
            data_root: PathBuf::from("/opt/sober/data"),
            blob_retention_days: 90,
            archive_grace_period_days: 30,
            worktree_stale_hours: 24,
            max_snapshots: 10,
        }
    }
}

impl WorkspaceDefaults {
    /// Create defaults with a custom data root (useful for testing).
    pub fn with_data_root(data_root: PathBuf) -> Self {
        Self {
            data_root,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_defaults_have_sane_values() {
        let defaults = WorkspaceDefaults::default();
        assert_eq!(defaults.data_root, PathBuf::from("/opt/sober/data"));
        assert_eq!(defaults.blob_retention_days, 90);
        assert_eq!(defaults.archive_grace_period_days, 30);
        assert_eq!(defaults.worktree_stale_hours, 24);
        assert_eq!(defaults.max_snapshots, 10);
    }

    #[test]
    fn custom_data_root() {
        let defaults = WorkspaceDefaults::with_data_root(PathBuf::from("/tmp/test"));
        assert_eq!(defaults.data_root, PathBuf::from("/tmp/test"));
        // Other defaults remain
        assert_eq!(defaults.blob_retention_days, 90);
    }
}
