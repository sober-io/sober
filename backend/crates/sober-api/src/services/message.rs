use sqlx::PgPool;

pub struct MessageService {
    pub(crate) db: PgPool,
}

impl MessageService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}
