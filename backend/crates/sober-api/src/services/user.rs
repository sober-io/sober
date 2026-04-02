use sqlx::PgPool;

pub struct UserService {
    pub(crate) db: PgPool,
}

impl UserService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}
