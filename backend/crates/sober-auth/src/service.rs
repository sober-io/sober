//! Authentication service — the core business logic for auth operations.
//!
//! [`AuthService`] orchestrates registration, login, logout, session
//! validation, and user management. It depends on repository traits
//! from `sober-core` and password hashing from `sober-crypto`.

use std::time::Instant;

use chrono::{Duration, Utc};
use metrics::{counter, histogram};
use sober_core::error::AppError;
use sober_core::types::{
    CreateSession, CreateUser, RoleKind, RoleRepo, SessionRepo, User, UserId, UserRepo, UserStatus,
};
use tracing::instrument;

use crate::error::AuthError;
use crate::extractor::AuthUser;
use crate::token;

/// Minimum password length enforced during registration.
const MIN_PASSWORD_LENGTH: usize = 12;

/// High-level authentication and authorization service.
///
/// Generic over the repository implementations. At binary startup,
/// concrete repo types are provided (e.g. `PgUserRepo`, `PgSessionRepo`).
pub struct AuthService<U, S, R>
where
    U: UserRepo,
    S: SessionRepo,
    R: RoleRepo,
{
    users: U,
    sessions: S,
    roles: R,
    session_ttl_seconds: u64,
}

impl<U, S, R> AuthService<U, S, R>
where
    U: UserRepo,
    S: SessionRepo,
    R: RoleRepo,
{
    /// Creates a new auth service with the given repositories and session TTL.
    pub fn new(users: U, sessions: S, roles: R, session_ttl_seconds: u64) -> Self {
        Self {
            users,
            sessions,
            roles,
            session_ttl_seconds,
        }
    }

    /// Registers a new user account.
    ///
    /// Validates the password length, hashes it with Argon2id, and creates
    /// the user with `Pending` status and the `user` role. An admin must
    /// approve the account before the user can log in.
    #[instrument(skip(self, password), fields(email = %email, username = %username))]
    pub async fn register(
        &self,
        email: &str,
        username: &str,
        password: &str,
    ) -> Result<User, AppError> {
        if password.len() < MIN_PASSWORD_LENGTH {
            return Err(AuthError::PasswordTooShort.into());
        }

        let password = password.to_owned();
        let password_hash =
            tokio::task::spawn_blocking(move || sober_crypto::password::hash_password(&password))
                .await
                .map_err(|e| AppError::Internal(e.into()))??;

        let input = CreateUser {
            email: email.to_owned(),
            username: username.to_owned(),
            password_hash,
        };

        self.users.create_with_roles(input, &[RoleKind::User]).await
    }

    /// Authenticates a user with email and password.
    ///
    /// Returns the raw session token (for the cookie) and the user.
    /// The raw token must never be logged or stored — only the hash is persisted.
    #[instrument(skip(self, password), fields(email = %email))]
    pub async fn login(&self, email: &str, password: &str) -> Result<(String, User), AppError> {
        let method = "password";
        let start = Instant::now();

        let user = self.users.get_by_email(email).await.map_err(|e| match e {
            AppError::NotFound(_) => {
                counter!("sober_auth_attempts_total", "method" => method, "status" => "failure")
                    .increment(1);
                histogram!("sober_auth_attempt_duration_seconds", "method" => method)
                    .record(start.elapsed().as_secs_f64());
                AppError::from(AuthError::InvalidCredentials)
            }
            other => other,
        })?;

        // Always run Argon2 verification before checking account status to
        // prevent timing oracles that leak whether an email is registered.
        let stored_hash = self.users.get_password_hash(user.id).await?;
        let password = password.to_owned();
        let valid = tokio::task::spawn_blocking(move || {
            sober_crypto::password::verify_password(&password, &stored_hash)
        })
        .await
        .map_err(|e| AppError::Internal(e.into()))??;

        if !valid {
            counter!("sober_auth_attempts_total", "method" => method, "status" => "failure")
                .increment(1);
            histogram!("sober_auth_attempt_duration_seconds", "method" => method)
                .record(start.elapsed().as_secs_f64());
            return Err(AuthError::InvalidCredentials.into());
        }

        if user.status != UserStatus::Active {
            counter!("sober_auth_attempts_total", "method" => method, "status" => "locked")
                .increment(1);
            histogram!("sober_auth_attempt_duration_seconds", "method" => method)
                .record(start.elapsed().as_secs_f64());
            return Err(AuthError::AccountNotActive.into());
        }

        let (raw_token, token_hash) = token::generate_session_token();
        let expires_at = Utc::now()
            + Duration::seconds(i64::try_from(self.session_ttl_seconds).unwrap_or(i64::MAX));

        let input = CreateSession {
            user_id: user.id,
            token_hash,
            expires_at,
        };
        self.sessions.create(input).await?;

        let elapsed = start.elapsed().as_secs_f64();
        counter!("sober_auth_attempts_total", "method" => method, "status" => "success")
            .increment(1);
        histogram!("sober_auth_attempt_duration_seconds", "method" => method).record(elapsed);
        counter!("sober_auth_sessions_created_total").increment(1);

        tracing::info!(user_id = %user.id, "user logged in");
        Ok((raw_token, user))
    }

    /// Validates a raw session token and returns the authenticated user.
    ///
    /// Hashes the token, looks it up in the session store, and loads the
    /// user's roles. Returns [`AuthUser`] on success.
    pub async fn validate_session(&self, raw_token: &str) -> Result<AuthUser, AppError> {
        let token_hash =
            token::hash_token(raw_token).map_err(|_| AppError::from(AuthError::SessionNotFound))?;

        let session = self
            .sessions
            .get_by_token_hash(&token_hash)
            .await?
            .ok_or(AppError::from(AuthError::SessionNotFound))?;

        let roles = self.roles.get_roles_for_user(session.user_id).await?;

        Ok(AuthUser {
            user_id: session.user_id,
            roles,
        })
    }

    /// Invalidates a session by deleting it from the store.
    ///
    /// Records `sober_auth_sessions_expired_total` on successful logout.
    #[instrument(skip(self, raw_token))]
    pub async fn logout(&self, raw_token: &str) -> Result<(), AppError> {
        let token_hash =
            token::hash_token(raw_token).map_err(|_| AppError::from(AuthError::SessionNotFound))?;

        self.sessions.delete_by_token_hash(&token_hash).await?;

        counter!("sober_auth_sessions_expired_total").increment(1);
        Ok(())
    }

    /// Approves a pending user account by setting its status to `Active`.
    #[instrument(skip(self))]
    pub async fn approve_user(&self, user_id: UserId) -> Result<(), AppError> {
        self.users.update_status(user_id, UserStatus::Active).await
    }

    /// Disables a user account by setting its status to `Disabled`.
    #[instrument(skip(self))]
    pub async fn disable_user(&self, user_id: UserId) -> Result<(), AppError> {
        self.users
            .update_status(user_id, UserStatus::Disabled)
            .await
    }
}
