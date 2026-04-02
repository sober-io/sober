use crate::state::AgentClient;
use sober_core::config::AppConfig;
use sqlx::PgPool;

#[allow(dead_code)]
pub struct EvolutionService {
    pub(crate) db: PgPool,
    pub(crate) agent_client: AgentClient,
    pub(crate) config: AppConfig,
}

impl EvolutionService {
    pub fn new(db: PgPool, agent_client: AgentClient, config: AppConfig) -> Self {
        Self {
            db,
            agent_client,
            config,
        }
    }
}
