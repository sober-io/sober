use sqlx::PgPool;

#[allow(dead_code)]
pub struct AuthApiService {
    pub(crate) db: PgPool,
}

impl AuthApiService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}
