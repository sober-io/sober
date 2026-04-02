use crate::connections::UserConnectionRegistry;
use sqlx::PgPool;

#[allow(dead_code)]
pub struct CollaboratorService {
    pub(crate) db: PgPool,
    pub(crate) user_connections: UserConnectionRegistry,
}

impl CollaboratorService {
    pub fn new(db: PgPool, user_connections: UserConnectionRegistry) -> Self {
        Self {
            db,
            user_connections,
        }
    }
}
