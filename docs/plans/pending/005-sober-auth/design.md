# 005 — sober-auth

**Date:** 2026-03-06

Authentication and authorization crate for Sober. Depends on `sober-crypto` (password hashing)
and `sober-core` (shared types, errors, config). v1 is password-only with admin-approved registration.

---

## Registration Flow

- User submits email, username, and password via `POST /auth/register`.
- Password validated (minimum 12 characters), then hashed via sober-crypto's Argon2id.
- User created with status `Pending` in the database.
- Response: 201 with message "Registration pending approval".
- Admin approves via `sober user approve <email>` CLI command, which changes status to `Active`.
- No email verification in v1 (no email provider configured).

## Login Flow

- User submits email and password via `POST /auth/login`.
- Look up user by email, then check account status:
  - `Pending` — return error "account pending approval".
  - `Disabled` — return error "account disabled".
  - `Active` — proceed.
- Verify password via sober-crypto.
- Generate a 256-bit random session token using `rand`.
- Hash the token with SHA-256 and store the hash in the `sessions` table with a configurable
  expiry (default: 30 days).
- Set an `HttpOnly`, `Secure`, `SameSite=Lax` cookie containing the raw token.
- Return 200 with user info.

## Session Validation (Middleware)

- Extract the session token from the cookie.
- SHA-256 hash the token and look it up — check moka in-memory cache first, fall back to PostgreSQL.
- Verify the session has not expired.
- Load user and roles from the database (cache in moka).
- Attach user context to request extensions via axum `Extension<AuthUser>`.
- `AuthUser` struct fields: `user_id`, `email`, `username`, `roles` (`Vec<String>`),
  `scopes` (`Vec<ScopeId>`).

## Logout

- Delete session from both the database and the moka in-memory cache.
- Clear the session cookie.

## RBAC

- Roles stored in `roles` and `user_roles` tables (see database schema).
- v1 defines two roles: `user` and `admin`.
- Permission checks via an axum extractor: `RequireRole("admin")` — returns 403 if the
  authenticated user lacks the required role.
- Scope resolution: when a user authenticates, resolve their permitted scopes (their own
  user scope plus any group scopes they belong to).

## Session Cleanup

- Expired sessions are cleaned up periodically by a background tokio task that runs every hour.
- On login, expired sessions for that specific user are also cleaned up.

## Error Type

`AuthError` enum with the following variants:

- `InvalidCredentials` — wrong email or password.
- `AccountPending` — account exists but has not been approved.
- `AccountDisabled` — account has been disabled by an admin.
- `SessionExpired` — session token is no longer valid.
- `InsufficientRole(String)` — user lacks the required role.

Each variant maps to the appropriate `AppError` variant (`Unauthorized`, `Forbidden`, etc.).

## Dependencies

| Crate | Purpose |
|-------|---------|
| `sober-crypto` | `hash_password`, `verify_password` |
| `sober-core` | `AppError`, shared types, config |
| `sqlx` | Database queries |
| `moka` | In-memory session/user cache |
| `rand` | Session token generation |
| `sha2` | Session token hashing |
| `axum` | Middleware, extractors |
| `tower` | `Layer`, `Service` for middleware |
| `axum-extra` | `CookieJar` for cookie handling |
