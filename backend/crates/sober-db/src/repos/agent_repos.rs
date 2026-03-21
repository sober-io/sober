//! Concrete bundle of all PostgreSQL repository implementations for the agent.

use sober_core::types::AgentRepos;
use sqlx::PgPool;

use super::{
    PgArtifactRepo, PgAuditLogRepo, PgConversationRepo, PgMessageRepo, PgPluginRepo, PgSecretRepo,
    PgUserRepo, PgWorkspaceRepo,
};

/// Bundles all Pg repository implementations required by the agent.
///
/// Construct once at binary startup with [`PgAgentRepos::new`] and pass to the
/// agent service. This avoids an unwieldy generic parameter list on the agent.
pub struct PgAgentRepos {
    messages: PgMessageRepo,
    conversations: PgConversationRepo,
    users: PgUserRepo,
    secrets: PgSecretRepo,
    audit_log: PgAuditLogRepo,
    artifacts: PgArtifactRepo,
    workspaces: PgWorkspaceRepo,
    plugins: PgPluginRepo,
}

impl PgAgentRepos {
    /// Creates all repositories backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self {
            messages: PgMessageRepo::new(pool.clone()),
            conversations: PgConversationRepo::new(pool.clone()),
            users: PgUserRepo::new(pool.clone()),
            secrets: PgSecretRepo::new(pool.clone()),
            audit_log: PgAuditLogRepo::new(pool.clone()),
            artifacts: PgArtifactRepo::new(pool.clone()),
            workspaces: PgWorkspaceRepo::new(pool.clone()),
            plugins: PgPluginRepo::new(pool),
        }
    }
}

impl AgentRepos for PgAgentRepos {
    type Msg = PgMessageRepo;
    type Conv = PgConversationRepo;
    type User = PgUserRepo;
    type Secret = PgSecretRepo;
    type Audit = PgAuditLogRepo;
    type Artifact = PgArtifactRepo;
    type Workspace = PgWorkspaceRepo;
    type Plg = PgPluginRepo;

    fn messages(&self) -> &PgMessageRepo {
        &self.messages
    }

    fn conversations(&self) -> &PgConversationRepo {
        &self.conversations
    }

    fn users(&self) -> &PgUserRepo {
        &self.users
    }

    fn secrets(&self) -> &PgSecretRepo {
        &self.secrets
    }

    fn audit_log(&self) -> &PgAuditLogRepo {
        &self.audit_log
    }

    fn artifacts(&self) -> &PgArtifactRepo {
        &self.artifacts
    }

    fn workspaces(&self) -> &PgWorkspaceRepo {
        &self.workspaces
    }

    fn plugins(&self) -> &PgPluginRepo {
        &self.plugins
    }
}
