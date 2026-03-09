//! PostgreSQL repository implementations.
//!
//! Each module contains a `Pg*Repo` struct implementing the corresponding
//! trait from `sober-core`. All repos take a `PgPool` and are constructed
//! at binary startup.

mod artifacts;
mod audit_log;
mod conversations;
mod jobs;
mod mcp_servers;
mod messages;
mod roles;
mod secrets;
mod sessions;
mod users;
mod workspace_repos;
mod workspaces;
mod worktrees;

pub use artifacts::PgArtifactRepo;
pub use audit_log::PgAuditLogRepo;
pub use conversations::PgConversationRepo;
pub use jobs::PgJobRepo;
pub use mcp_servers::PgMcpServerRepo;
pub use messages::PgMessageRepo;
pub use roles::PgRoleRepo;
pub use secrets::PgSecretRepo;
pub use sessions::PgSessionRepo;
pub use users::PgUserRepo;
pub use workspace_repos::PgWorkspaceRepoRepo;
pub use workspaces::PgWorkspaceRepo;
pub use worktrees::PgWorktreeRepo;
