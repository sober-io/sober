use sqlx::PgPool;

pub struct TagService {
    pub(crate) db: PgPool,
}

impl TagService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}
