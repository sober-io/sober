//! Authentication route handlers.
//!
//! Thin handlers that delegate to [`AuthService`] from `sober-auth`.

use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use http::HeaderValue;
use http::header::SET_COOKIE;
use sober_auth::{AuthUser, cookie_name};
use sober_core::error::AppError;
use sober_core::types::ApiResponse;

use crate::services::auth::UserProfile;
use crate::state::AppState;

/// Returns the auth routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
}

/// Request body for `POST /auth/register`.
#[derive(serde::Deserialize)]
struct RegisterRequest {
    email: String,
    username: String,
    password: String,
}

/// `POST /api/v1/auth/register` — create a new user account.
async fn register(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RegisterRequest>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user = state
        .auth
        .register(&body.email, &body.username, &body.password)
        .await?;

    // Create inbox conversation for the new user.
    state.auth_service.create_inbox_for_user(user.id).await?;

    Ok(ApiResponse::new(serde_json::json!({
        "id": user.id.to_string(),
        "email": user.email,
        "username": user.username,
        "status": format!("{:?}", user.status),
    })))
}

/// Request body for `POST /auth/login`.
#[derive(serde::Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

/// `POST /api/v1/auth/login` — authenticate and receive a session token.
async fn login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> Result<
    (
        [(http::header::HeaderName, HeaderValue); 1],
        ApiResponse<serde_json::Value>,
    ),
    AppError,
> {
    let (token, user) = state.auth.login(&body.email, &body.password).await?;

    let cookie = format!(
        "{}={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        cookie_name(),
        token,
        state.config.auth.session_ttl_seconds,
    );
    let cookie_header = HeaderValue::from_str(&cookie).map_err(|e| AppError::Internal(e.into()))?;

    Ok((
        [(SET_COOKIE, cookie_header)],
        ApiResponse::new(serde_json::json!({
            "token": token,
            "user": {
                "id": user.id.to_string(),
                "email": user.email,
                "username": user.username,
            },
        })),
    ))
}

/// `POST /api/v1/auth/logout` — invalidate the current session.
async fn logout(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    req: axum::extract::Request,
) -> Result<
    (
        [(http::header::HeaderName, HeaderValue); 1],
        ApiResponse<serde_json::Value>,
    ),
    AppError,
> {
    let raw_token = extract_raw_token(&req).ok_or(AppError::Unauthorized)?;
    let _ = auth_user;
    state.auth.logout(&raw_token).await?;

    let clear_cookie = format!(
        "{}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0",
        cookie_name(),
    );
    let cookie_header =
        HeaderValue::from_str(&clear_cookie).map_err(|e| AppError::Internal(e.into()))?;

    Ok((
        [(SET_COOKIE, cookie_header)],
        ApiResponse::new(serde_json::json!({ "logged_out": true })),
    ))
}

/// `GET /api/v1/auth/me` — returns the current authenticated user with roles.
async fn me(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
) -> Result<ApiResponse<UserProfile>, AppError> {
    let profile = state
        .auth_service
        .get_user_with_roles(auth_user.user_id)
        .await?;
    Ok(ApiResponse::new(profile))
}

/// Extracts the raw session token from the request (Bearer header or cookie).
fn extract_raw_token(req: &axum::extract::Request) -> Option<String> {
    if let Some(auth_header) = req.headers().get(http::header::AUTHORIZATION)
        && let Ok(val) = auth_header.to_str()
        && let Some(token) = val.strip_prefix("Bearer ")
    {
        let token = token.trim();
        if !token.is_empty() {
            return Some(token.to_owned());
        }
    }

    if let Some(cookie_header) = req.headers().get(http::header::COOKIE)
        && let Ok(val) = cookie_header.to_str()
    {
        for pair in val.split("; ") {
            if let Some((name, value)) = pair.split_once('=')
                && name.trim() == cookie_name()
            {
                let v = value.trim();
                let v = v
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or(v);
                return Some(v.to_owned());
            }
        }
    }

    None
}
