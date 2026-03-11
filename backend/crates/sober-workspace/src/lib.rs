//! Workspace business logic for the Sober agent system.
//!
//! This crate owns filesystem layout, git operations (via git2), blob storage,
//! workspace config parsing, snapshot management, and worktree management.
//! Database operations (`Pg*Repo`) live in `sober-db`, not here.

pub mod blob;
pub mod config;
pub mod error;
pub mod fs;
pub mod snapshot;
pub mod worktree;

pub use blob::BlobStore;
pub use config::WorkspaceDefaults;
pub use error::WorkspaceError;
pub use fs::init_workspace_dir;
pub use snapshot::SnapshotManager;
pub use worktree::{create_git_worktree, list_git_worktrees, remove_git_worktree};
