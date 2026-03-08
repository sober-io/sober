# 006 — sober-auth

**Date:** 2026-03-06
**Updated:** 2026-03-08

Authentication and authorization crate for Sober. Depends on `sober-crypto` (password hashing)
and `sober-core` (shared types, errors, config). v1 is password-only with admin-approved registration.

---

## Dependency Injection: Generics over Trait Objects

Repository traits in `sober-core` use RPITIT (Return Position Impl Trait In Traits):

```rust
pub trait UserRepo: Send + Sync {
    fn get_by_email(&self, email: &str) -> impl Future<Output = Result<User, AppError>> + Send;
    // ...
}
```

RPITIT traits are **not dyn-compatible** — `Arc<dyn UserRepo>` won't compile because the
compiler cannot construct a vtable for methods returning `impl Trait`. Therefore, `AuthService`
uses generics instead of trait objects:

```rust
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
```

Binary crates construct concrete repos and pass them as type parameters:

```rust
let auth_service = AuthService::new(
    PgUserRepo::new(pool.clone()),
    PgSessionRepo::new(pool.clone()),
    PgRoleRepo::new(pool.clone()),
    config.auth.session_ttl_seconds,
);
```

This pattern applies to all crates consuming repo traits from `sober-core`.

---

## Registration Flow

- User submits email, username, and password via `POST /auth/register`.
- Password validated (minimum 12 characters), then hashed via sober-crypto's Argon2id
  (delegated to `spawn_blocking` to avoid blocking the async runtime).
- User created with status `Pending` and `user` role in the database.
- Response: 201 with message "Registration pending approval".
- Admin approves via `sober user approve <email>` CLI command, which changes status to `Active`.
- No email verification in v1 (no email provider configured).

## Login Flow

- User submits email and password via `POST /auth/login`.
- Look up user by email. If not found, return `InvalidCredentials`.
- **Always run Argon2 password verification first** to prevent timing oracles that
  leak email existence via response time differences.
- After password verification, check account status:
  - `Pending` or `Disabled` — return error "account not active".
  - `Active` — proceed.
- Generate a 256-bit random session token using `rand_core` (OsRng).
- Hash the token with SHA-256 and store the hash in the `sessions` table with a configurable
  expiry (default: 30 days).
- Set an `HttpOnly`, `Secure`, `SameSite=Lax` cookie containing the raw hex token.
- Return 200 with user info.

## Session Validation (Middleware)

- `AuthLayer` is a tower `Layer` that wraps services with `AuthMiddleware`.
- `AuthMiddleware` extracts the session token synchronously (before the async
  boundary, since `Body` is not `Sync`), then validates via `AuthService::validate_session`.
- Token extraction priority: `Authorization: Bearer <token>` header first, falls back
  to `sober_session` cookie. This supports both browser clients (cookie) and programmatic
  clients (CLI, bots, API consumers) via Bearer token.
- On success, inserts `AuthUser` into request extensions. On failure, continues without
  inserting anything — downstream extractors handle the 401.
- `AuthUser` struct fields: `user_id: UserId`, `roles: Vec<RoleKind>`.
  Minimal — avoids extra DB lookups per request. Email/username can be loaded on demand.
- Cookie values with RFC 6265 quoted-string wrapping (double quotes) are stripped.
- `cookie_name()` is exported for use by route handlers when setting/clearing cookies.

## RoleKind Enum

`RoleKind` is a type-safe representation of authorization roles with known variants and
extensibility for custom roles:

```rust
pub enum RoleKind {
    User,
    Admin,
    Custom(String),  // future custom roles
}
```

Serializes as a lowercase string (`"user"`, `"admin"`, `"moderator"`). Converts from
database role name strings via `From<String>`.

## RoleRepo trait

`RoleRepo` was added to `sober-core` alongside the existing repo traits:

```rust
pub trait RoleRepo: Send + Sync {
    fn get_roles_for_user(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<RoleKind>, AppError>> + Send;
}
```

Implemented as `PgRoleRepo` in `sober-db`, joining `roles` + `user_roles` tables
filtered by `GLOBAL` scope. Role name strings are converted to `RoleKind` variants.

## Logout

- Hash the raw token, delete session from the database via `SessionRepo::delete_by_token_hash()`.
- Route handler (in sober-api) clears the session cookie.

## RBAC

- Roles stored in `roles` and `user_roles` tables (see database schema).
- v1 defines two roles: `user` and `admin`.
- `RequireAdmin` axum extractor — extracts `AuthUser`, checks for `admin` role,
  returns 403 `Forbidden` if missing. Wraps the inner `AuthUser` as `RequireAdmin(AuthUser)`.
- Note: Rust does not support `&'static str` as a const generic parameter, so a generic
  `RequireRole<const ROLE: &str>` is not possible. Concrete extractors per role are used instead.
- Scope resolution deferred to a later plan when group-scoped access is needed.

## Session Cleanup

- Expired sessions are cleaned up periodically by a `sober-scheduler` job (see plan 016).
  `sober-auth` does not own the scheduling.
- Session expiry is enforced in the `SessionRepo::get_by_token_hash()` SQL query
  (`WHERE expires_at > NOW()`), so expired sessions are rejected even if not yet cleaned up.

## Error Type

`AuthError` enum with the following variants:

- `PasswordTooShort` — password under 12 characters.
- `InvalidCredentials` — wrong email or password.
- `AccountNotActive` — account is pending or disabled.
- `SessionNotFound` — session token is invalid or expired.
- `InsufficientRole(RoleKind)` — user lacks the required role.

Each variant maps to the appropriate `AppError` variant (`Unauthorized`, `Forbidden`, etc.).

## Module Structure

| Module | Purpose |
|--------|---------|
| `service.rs` | `AuthService` — register, login, logout, validate_session, approve/disable user |
| `middleware.rs` | `AuthLayer`, `AuthMiddleware`, `cookie_name()`, Bearer + cookie token extraction |
| `extractor.rs` | `AuthUser`, `RequireAdmin` axum extractors |
| `error.rs` | `AuthError` enum, `From<AuthError> for AppError` |
| `token.rs` | `generate_session_token()`, `hash_token()` |
| `lib.rs` | Re-exports: `AuthError`, `AuthUser`, `RequireAdmin`, `AuthLayer`, `cookie_name`, `AuthService` |

## Dependencies

| Crate | Purpose |
|-------|---------|
| `sober-crypto` | `hash_password`, `verify_password` (Argon2id) |
| `sober-core` | `AppError`, shared types, config, repo traits (`UserRepo`, `SessionRepo`, `RoleRepo`) |
| `rand_core` | Session token generation (OsRng) |
| `sha2` | Session token SHA-256 hashing |
| `hex` | Hex encoding/decoding for tokens |
| `axum` / `axum-core` | Extractors (`FromRequestParts`) |
| `http` | Request types, headers |
| `tower` | `Layer`, `Service` for middleware |
| `chrono` | Session expiry timestamps |
| `tokio` | `spawn_blocking` for Argon2 |
| `tracing` | Structured logging |
