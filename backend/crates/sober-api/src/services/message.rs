use sqlx::PgPool;

#[allow(dead_code)]
pub struct MessageService {
    pub(crate) db: PgPool,
}

impl MessageService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}
