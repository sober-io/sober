//! Authentication, session management, and RBAC authorization for the Sober
//! system.
//!
//! This crate provides:
//!
//! - [`AuthService`] — registration, login, logout, session validation, and
//!   user management.
//! - [`AuthLayer`] — tower middleware that validates session cookies and
//!   inserts [`AuthUser`] into request extensions.
//! - [`AuthUser`] — the authenticated user context, extracted from request
//!   extensions by handlers.
//! - [`RequireAdmin`] — an axum extractor that requires the admin role.
//! - [`AuthError`] — auth-specific error types that map to [`AppError`](sober_core::error::AppError).
//!
//! # Architecture
//!
//! `sober-auth` depends on `sober-core` (for repo traits and domain types)
//! and `sober-crypto` (for password hashing). It does NOT depend on
//! `sober-db` — concrete repo implementations are injected via generics.

pub mod error;
pub mod extractor;
pub mod middleware;
pub mod service;
mod token;

pub use error::AuthError;
pub use extractor::{AuthUser, RequireAdmin};
pub use middleware::{AuthLayer, cookie_name};
pub use service::AuthService;
