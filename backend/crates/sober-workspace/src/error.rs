//! Workspace-specific error types.

use sober_core::AppError;

/// Errors that can occur during workspace operations.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    /// Workspace not found.
    #[error("workspace not found: {0}")]
    NotFound(String),

    /// Workspace already exists.
    #[error("workspace already exists: {0}")]
    AlreadyExists(String),

    /// Workspace is archived and cannot be modified.
    #[error("workspace is archived")]
    Archived,

    /// Repository not found.
    #[error("repo not found: {0}")]
    RepoNotFound(String),

    /// Worktree branch conflict.
    #[error("worktree conflict: branch '{branch}' already checked out by {held_by}")]
    WorktreeConflict {
        /// The conflicting branch name.
        branch: String,
        /// Who holds the branch.
        held_by: String,
    },

    /// Filesystem I/O error.
    #[error("filesystem error: {0}")]
    Filesystem(#[source] std::io::Error),

    /// Git operation failed.
    #[error("git error: {0}")]
    Git(#[from] git2::Error),

    /// Snapshot operation failed.
    #[error("snapshot error: {0}")]
    Snapshot(String),

    /// Internal processing error.
    #[error("{0}")]
    Internal(String),

    /// Invalid state transition.
    #[error("invalid state transition: {from} -> {to}")]
    InvalidStateTransition {
        /// Current state.
        from: String,
        /// Attempted target state.
        to: String,
    },
}

impl From<WorkspaceError> for AppError {
    fn from(err: WorkspaceError) -> Self {
        match err {
            WorkspaceError::NotFound(msg) => AppError::NotFound(msg),
            WorkspaceError::AlreadyExists(msg) => AppError::Conflict(msg),
            WorkspaceError::Archived => AppError::Validation("workspace is archived".into()),
            WorkspaceError::RepoNotFound(msg) => AppError::NotFound(msg),
            WorkspaceError::WorktreeConflict { branch, held_by } => AppError::Conflict(format!(
                "branch '{branch}' already checked out by {held_by}"
            )),
            WorkspaceError::InvalidStateTransition { from, to } => {
                AppError::Validation(format!("invalid state transition: {from} -> {to}"))
            }
            WorkspaceError::Internal(_)
            | WorkspaceError::Filesystem(_)
            | WorkspaceError::Git(_)
            | WorkspaceError::Snapshot(_) => AppError::Internal(err.into()),
        }
    }
}
