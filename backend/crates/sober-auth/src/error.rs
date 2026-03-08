//! Authentication error types.
//!
//! [`AuthError`] covers all auth-specific failure modes. Each variant maps
//! to an appropriate [`AppError`] variant via the [`From`] implementation.

use sober_core::error::AppError;
use sober_core::types::RoleKind;

/// Authentication and authorization errors.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// Password does not meet the minimum length requirement.
    #[error("password must be at least 12 characters")]
    PasswordTooShort,

    /// Email or password is incorrect.
    #[error("invalid credentials")]
    InvalidCredentials,

    /// Account exists but is not in the `Active` state.
    #[error("account is not active")]
    AccountNotActive,

    /// Session token is missing, expired, or not found.
    #[error("session expired or not found")]
    SessionNotFound,

    /// User does not hold the required role.
    #[error("insufficient role: {0}")]
    InsufficientRole(RoleKind),
}

impl From<AuthError> for AppError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::PasswordTooShort => AppError::Validation(err.to_string()),
            AuthError::InvalidCredentials => AppError::Unauthorized,
            AuthError::AccountNotActive => AppError::Forbidden,
            AuthError::SessionNotFound => AppError::Unauthorized,
            AuthError::InsufficientRole(_) => AppError::Forbidden,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_too_short_maps_to_validation() {
        let app_err: AppError = AuthError::PasswordTooShort.into();
        assert!(matches!(app_err, AppError::Validation(_)));
    }

    #[test]
    fn invalid_credentials_maps_to_unauthorized() {
        let app_err: AppError = AuthError::InvalidCredentials.into();
        assert!(matches!(app_err, AppError::Unauthorized));
    }

    #[test]
    fn account_not_active_maps_to_forbidden() {
        let app_err: AppError = AuthError::AccountNotActive.into();
        assert!(matches!(app_err, AppError::Forbidden));
    }

    #[test]
    fn session_not_found_maps_to_unauthorized() {
        let app_err: AppError = AuthError::SessionNotFound.into();
        assert!(matches!(app_err, AppError::Unauthorized));
    }

    #[test]
    fn insufficient_role_maps_to_forbidden() {
        let app_err: AppError = AuthError::InsufficientRole(RoleKind::Admin).into();
        assert!(matches!(app_err, AppError::Forbidden));
    }
}
