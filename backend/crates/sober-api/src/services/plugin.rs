use crate::state::AgentClient;
use sqlx::PgPool;

pub struct PluginService {
    pub(crate) db: PgPool,
    pub(crate) agent_client: AgentClient,
}

impl PluginService {
    pub fn new(db: PgPool, agent_client: AgentClient) -> Self {
        Self { db, agent_client }
    }
}
