//! PostgreSQL access layer for the Sõber system.
//!
//! This crate is the **only** place that depends on `sqlx`. It provides:
//!
//! - [`create_pool`] — connection pool creation with consistent settings
//! - `Pg*Repo` structs — concrete implementations of repo traits from `sober-core`
//!
//! # Wiring at Startup
//!
//! Binaries construct the pool and repos, then pass them as trait objects:
//!
//! ```ignore
//! let pool = sober_db::create_pool(&config).await?;
//! let user_repo: Arc<dyn UserRepo> = Arc::new(PgUserRepo::new(pool.clone()));
//! ```

pub mod pool;
pub mod repos;
mod rows;

pub use pool::{DatabaseConfig, create_pool, create_pool_with_service};
pub use repos::{
    PgAgentRepos, PgArtifactRepo, PgAuditLogRepo, PgConversationRepo, PgConversationUserRepo,
    PgJobRepo, PgJobRunRepo, PgMcpServerRepo, PgMessageRepo, PgRoleRepo, PgSecretRepo,
    PgSessionRepo, PgTagRepo, PgUserRepo, PgWorkspaceRepo, PgWorkspaceRepoRepo, PgWorktreeRepo,
};
