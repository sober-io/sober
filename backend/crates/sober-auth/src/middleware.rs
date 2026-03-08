//! Authentication middleware for axum.
//!
//! [`AuthLayer`] is a tower [`Layer`] that extracts the session token from
//! either the `Authorization: Bearer` header (preferred) or the `sober_session`
//! cookie (fallback), validates it via [`AuthService`], and inserts [`AuthUser`]
//! into request extensions. If no valid session is found, the request proceeds
//! without an [`AuthUser`] — downstream extractors handle the 401 response.

use std::sync::Arc;
use std::task::{Context, Poll};

use axum_core::body::Body;
use http::Request;
use sober_core::types::{RoleRepo, SessionRepo, UserRepo};
use tower::{Layer, Service};

use crate::service::AuthService;

/// Session cookie name.
const COOKIE_NAME: &str = "sober_session";

/// Tower [`Layer`] that adds authentication to a service.
///
/// Wraps an inner service with [`AuthMiddleware`], which validates the
/// session cookie on every request and inserts [`AuthUser`] into
/// request extensions on success.
#[derive(Clone)]
pub struct AuthLayer<U, S, R>
where
    U: UserRepo,
    S: SessionRepo,
    R: RoleRepo,
{
    auth_service: Arc<AuthService<U, S, R>>,
}

impl<U, S, R> AuthLayer<U, S, R>
where
    U: UserRepo,
    S: SessionRepo,
    R: RoleRepo,
{
    /// Creates a new auth layer backed by the given service.
    pub fn new(auth_service: Arc<AuthService<U, S, R>>) -> Self {
        Self { auth_service }
    }
}

impl<Svc, U, S, R> Layer<Svc> for AuthLayer<U, S, R>
where
    U: UserRepo,
    S: SessionRepo,
    R: RoleRepo,
{
    type Service = AuthMiddleware<Svc, U, S, R>;

    fn layer(&self, inner: Svc) -> Self::Service {
        AuthMiddleware {
            inner,
            auth_service: self.auth_service.clone(),
        }
    }
}

/// Tower service that validates session tokens and inserts [`AuthUser`].
///
/// On each request:
/// 1. Checks the `Authorization: Bearer <token>` header first.
/// 2. Falls back to the `sober_session` cookie if no `Authorization` header.
/// 3. If a token is found, validates it via [`AuthService::validate_session`].
/// 4. On success, inserts [`AuthUser`] into request extensions.
/// 5. On failure or absence, continues without inserting anything.
/// 6. Always calls the inner service.
#[derive(Clone)]
pub struct AuthMiddleware<Svc, U, S, R>
where
    U: UserRepo,
    S: SessionRepo,
    R: RoleRepo,
{
    inner: Svc,
    auth_service: Arc<AuthService<U, S, R>>,
}

impl<Svc, U, S, R> Service<Request<Body>> for AuthMiddleware<Svc, U, S, R>
where
    Svc: Service<Request<Body>> + Clone + Send + 'static,
    Svc::Future: Send,
    U: UserRepo + 'static,
    S: SessionRepo + 'static,
    R: RoleRepo + 'static,
{
    type Response = Svc::Response;
    type Error = Svc::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let auth_service = self.auth_service.clone();
        let mut inner = self.inner.clone();

        // Extract the token synchronously before the async boundary.
        // Checks Authorization header first, falls back to cookie.
        let raw_token = extract_bearer_token(&req).or_else(|| extract_cookie(&req));

        Box::pin(async move {
            if let Some(token) = raw_token {
                match auth_service.validate_session(&token).await {
                    Ok(auth_user) => {
                        req.extensions_mut().insert(auth_user);
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "session validation failed");
                    }
                }
            }

            inner.call(req).await
        })
    }
}

/// Extracts the raw session token from the `Authorization: Bearer <token>` header.
fn extract_bearer_token(req: &Request<Body>) -> Option<String> {
    let auth_header = req
        .headers()
        .get(http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;

    let token = auth_header.strip_prefix("Bearer ")?;
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    Some(token.to_owned())
}

/// Extracts the raw session token from the `Cookie` header.
fn extract_cookie(req: &Request<Body>) -> Option<String> {
    let cookie_header = req.headers().get(http::header::COOKIE)?.to_str().ok()?;

    cookie_header.split("; ").find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        if name.trim() == COOKIE_NAME {
            let v = value.trim();
            // Strip optional RFC 6265 quoted-string wrapping.
            let v = v
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(v);
            Some(v.to_owned())
        } else {
            None
        }
    })
}

/// Returns the session cookie name used by the auth middleware.
///
/// Useful for setting and clearing the cookie in HTTP responses.
#[must_use]
pub const fn cookie_name() -> &'static str {
    COOKIE_NAME
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request_with_header(name: http::header::HeaderName, value: &str) -> Request<Body> {
        Request::builder()
            .header(name, value)
            .body(Body::empty())
            .unwrap()
    }

    #[test]
    fn extract_bearer_token_valid() {
        let req = make_request_with_header(http::header::AUTHORIZATION, "Bearer mytoken123");
        assert_eq!(extract_bearer_token(&req).as_deref(), Some("mytoken123"));
    }

    #[test]
    fn extract_bearer_token_missing_header() {
        let req = Request::builder().body(Body::empty()).unwrap();
        assert!(extract_bearer_token(&req).is_none());
    }

    #[test]
    fn extract_bearer_token_wrong_scheme() {
        let req = make_request_with_header(http::header::AUTHORIZATION, "Basic abc123");
        assert!(extract_bearer_token(&req).is_none());
    }

    #[test]
    fn extract_bearer_token_empty_token() {
        let req = make_request_with_header(http::header::AUTHORIZATION, "Bearer ");
        assert!(extract_bearer_token(&req).is_none());
    }

    #[test]
    fn extract_cookie_valid() {
        let req = make_request_with_header(http::header::COOKIE, "sober_session=abc123");
        assert_eq!(extract_cookie(&req).as_deref(), Some("abc123"));
    }

    #[test]
    fn extract_cookie_among_multiple() {
        let req = make_request_with_header(
            http::header::COOKIE,
            "other=val; sober_session=tok456; another=x",
        );
        assert_eq!(extract_cookie(&req).as_deref(), Some("tok456"));
    }

    #[test]
    fn extract_cookie_missing() {
        let req = make_request_with_header(http::header::COOKIE, "other=val");
        assert!(extract_cookie(&req).is_none());
    }

    #[test]
    fn bearer_takes_priority_over_cookie() {
        let req = Request::builder()
            .header(http::header::AUTHORIZATION, "Bearer bearer_token")
            .header(http::header::COOKIE, "sober_session=cookie_token")
            .body(Body::empty())
            .unwrap();
        let token = extract_bearer_token(&req).or_else(|| extract_cookie(&req));
        assert_eq!(token.as_deref(), Some("bearer_token"));
    }
}
