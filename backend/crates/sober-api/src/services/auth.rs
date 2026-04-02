use serde::Serialize;
use sober_core::error::AppError;
use sober_core::types::{ConversationRepo, RoleKind, RoleRepo, UserId, UserRepo};
use sober_db::{PgConversationRepo, PgRoleRepo, PgUserRepo};
use sqlx::PgPool;

/// Typed response for user profile.
#[derive(Serialize)]
pub struct UserProfile {
    pub id: String,
    pub email: String,
    pub username: String,
    pub status: String,
    pub roles: Vec<String>,
}

pub struct AuthApiService {
    db: PgPool,
}

impl AuthApiService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// Create an inbox conversation for a newly registered user.
    pub async fn create_inbox_for_user(&self, user_id: UserId) -> Result<(), AppError> {
        let conv_repo = PgConversationRepo::new(self.db.clone());
        conv_repo.create_inbox(user_id).await?;
        Ok(())
    }

    /// Get user profile with roles for /me endpoint.
    pub async fn get_user_with_roles(&self, user_id: UserId) -> Result<UserProfile, AppError> {
        let user_repo = PgUserRepo::new(self.db.clone());
        let role_repo = PgRoleRepo::new(self.db.clone());

        let user = user_repo.get_by_id(user_id).await?;
        let roles = role_repo
            .get_roles_for_user(user_id)
            .await
            .unwrap_or_default();
        let role_names: Vec<String> = roles
            .iter()
            .map(|r| match r {
                RoleKind::User => "user".to_string(),
                RoleKind::Admin => "admin".to_string(),
                RoleKind::Custom(name) => name.clone(),
            })
            .collect();

        Ok(UserProfile {
            id: user.id.to_string(),
            email: user.email,
            username: user.username,
            status: format!("{:?}", user.status),
            roles: role_names,
        })
    }
}
