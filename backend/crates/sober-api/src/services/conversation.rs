use sober_core::config::AppConfig;
use sqlx::PgPool;

pub struct ConversationService {
    pub(crate) db: PgPool,
    pub(crate) config: AppConfig,
}

impl ConversationService {
    pub fn new(db: PgPool, config: AppConfig) -> Self {
        Self { db, config }
    }
}
