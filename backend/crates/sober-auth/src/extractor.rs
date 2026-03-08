//! Axum extractors for authentication and authorization.
//!
//! [`AuthUser`] represents an authenticated user, placed in request
//! extensions by [`AuthLayer`](crate::AuthLayer). [`RequireAdmin`] is a
//! wrapper extractor that additionally checks for the `admin` role.

use axum_core::extract::FromRequestParts;
use http::request::Parts;
use sober_core::error::AppError;
use sober_core::types::UserId;

use crate::error::AuthError;

/// An authenticated user attached to a request by the auth middleware.
///
/// Placed into request extensions by [`AuthLayer`](crate::AuthLayer).
/// Handlers can extract this directly to require authentication, or
/// use [`RequireAdmin`] to additionally require the admin role.
#[derive(Debug, Clone)]
pub struct AuthUser {
    /// The user's unique identifier.
    pub user_id: UserId,
    /// Role names assigned to this user (e.g. `["user", "admin"]`).
    pub roles: Vec<String>,
}

impl AuthUser {
    /// Returns `true` if the user holds the given role.
    #[must_use]
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthUser>()
            .cloned()
            .ok_or_else(|| AuthError::SessionNotFound.into())
    }
}

/// Axum extractor that requires the authenticated user to hold the `admin` role.
///
/// Returns 401 if the user is not authenticated, or 403 if the user lacks
/// the `admin` role.
///
/// # Usage
///
/// ```ignore
/// async fn admin_handler(RequireAdmin(user): RequireAdmin) {
///     // `user` is the authenticated AuthUser with admin role
/// }
/// ```
pub struct RequireAdmin(pub AuthUser);

impl<S> FromRequestParts<S> for RequireAdmin
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if user.has_role("admin") {
            Ok(RequireAdmin(user))
        } else {
            Err(AuthError::InsufficientRole("admin".to_owned()).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sober_core::types::UserId;

    #[test]
    fn has_role_returns_true_for_matching_role() {
        let user = AuthUser {
            user_id: UserId::new(),
            roles: vec!["user".into(), "admin".into()],
        };
        assert!(user.has_role("admin"));
        assert!(user.has_role("user"));
    }

    #[test]
    fn has_role_returns_false_for_missing_role() {
        let user = AuthUser {
            user_id: UserId::new(),
            roles: vec!["user".into()],
        };
        assert!(!user.has_role("admin"));
    }
}
