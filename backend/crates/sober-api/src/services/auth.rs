use sqlx::PgPool;

pub struct AuthApiService {
    pub(crate) db: PgPool,
}

impl AuthApiService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}
