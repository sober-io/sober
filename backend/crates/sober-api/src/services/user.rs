use serde::Serialize;
use sober_core::error::AppError;
use sober_core::types::UserRepo;
use sober_db::PgUserRepo;
use sqlx::PgPool;
use tracing::instrument;

/// Typed response for user search results.
#[derive(Serialize)]
pub struct UserSearchResult {
    pub id: String,
    pub username: String,
}

pub struct UserService {
    db: PgPool,
}

impl UserService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// Search users by username prefix.
    #[instrument(level = "debug", skip(self))]
    pub async fn search(&self, query: &str, limit: i64) -> Result<Vec<UserSearchResult>, AppError> {
        let repo = PgUserRepo::new(self.db.clone());
        let users = repo.search_by_username(query, limit).await?;
        Ok(users
            .into_iter()
            .map(|u| UserSearchResult {
                id: u.id.to_string(),
                username: u.username,
            })
            .collect())
    }
}
