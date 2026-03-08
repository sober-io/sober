//! Authentication middleware for axum.
//!
//! [`AuthLayer`] is a tower [`Layer`] that extracts the session cookie,
//! validates it via [`AuthService`], and inserts [`AuthUser`] into request
//! extensions. If no valid session is found, the request proceeds without
//! an [`AuthUser`] — downstream extractors handle the 401 response.

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

/// Tower service that validates session cookies and inserts [`AuthUser`].
///
/// On each request:
/// 1. Reads the `Cookie` header and finds the `sober_session` cookie.
/// 2. If found, validates it via [`AuthService::validate_session`].
/// 3. On success, inserts [`AuthUser`] into request extensions.
/// 4. On failure or absence, continues without inserting anything.
/// 5. Always calls the inner service.
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

        // Extract the cookie value synchronously before the async boundary.
        // This avoids borrowing the non-Sync request body across an await.
        let raw_token = extract_cookie(&req);

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
