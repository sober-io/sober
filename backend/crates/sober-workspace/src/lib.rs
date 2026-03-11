//! Workspace business logic for the Sober agent system.
//!
//! This crate owns filesystem layout, blob storage, workspace config parsing,
//! and worktree management. Database operations (`Pg*Repo`) live in `sober-db`,
//! not here.

pub mod blob;
pub mod error;
pub mod fs;

pub use blob::BlobStore;
pub use error::WorkspaceError;
pub use fs::init_workspace_dir;
