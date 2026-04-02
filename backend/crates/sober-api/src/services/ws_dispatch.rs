use crate::connections::ConnectionRegistry;
use crate::state::AgentClient;
use sqlx::PgPool;

#[allow(dead_code)]
pub struct WsDispatchService {
    pub(crate) db: PgPool,
    pub(crate) agent_client: AgentClient,
    pub(crate) connections: ConnectionRegistry,
}

impl WsDispatchService {
    pub fn new(db: PgPool, agent_client: AgentClient, connections: ConnectionRegistry) -> Self {
        Self {
            db,
            agent_client,
            connections,
        }
    }
}
