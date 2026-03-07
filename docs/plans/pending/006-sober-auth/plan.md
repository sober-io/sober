# 006 — sober-auth: Implementation Plan

**Date:** 2026-03-06

---

## Steps

1. **Add dependencies to Cargo.toml.**
   Add `sober-crypto`, `sober-core` as workspace dependencies. Add `sqlx`, `rand`,
   `sha2`, `axum`, `axum-extra`, `tower`, `tokio`, `serde`, `thiserror`, `tracing`.

2. **Create module structure.**
   - `src/lib.rs` — public API, re-exports.
   - `src/error.rs` — `AuthError` enum.
   - `src/password.rs` — password validation and hashing wrapper.
   - `src/session.rs` — session creation, validation, deletion, cleanup.
   - `src/middleware.rs` — auth middleware (tower Layer).
   - `src/rbac.rs` — role-based access control extractor and scope resolution.
   - `src/service.rs` — high-level auth service functions.

3. **Implement `error.rs`.**
   Define `AuthError` enum with variants: `InvalidCredentials`, `AccountPending`,
   `AccountDisabled`, `SessionExpired`, `InsufficientRole(String)`.
   Implement `From<AuthError> for AppError`.

4. **Implement `password.rs`.**
   Thin wrapper around sober-crypto that validates password policy (minimum 12 characters)
   before delegating to `sober_crypto::hash_password` and `sober_crypto::verify_password`.

5. **Implement `session.rs`.**
   - Define `SessionStore` trait with `create`, `validate`, `delete`, `cleanup_expired` methods.
   - Implement `PgSessionStore` backed by PostgreSQL (the default and only v1 implementation).
   - `create_session` — generate 256-bit random token, SHA-256 hash it, store hash in DB,
     return raw token.
   - `validate_session` — accept raw token, hash it, query DB, verify expiry, return session
     metadata.
   - `delete_session` — remove from DB.
   - `cleanup_expired` — delete all expired sessions from DB.
   - The `SessionStore` trait allows swapping in a caching layer (e.g., moka or Redis) later
     without changing callers.

6. **Implement `middleware.rs`.**
   - `AuthMiddleware` as a tower `Layer` that extracts the session cookie, validates the
     session, and attaches `AuthUser` to request extensions.
   - Optional variant (`OptionalAuthMiddleware`) that allows unauthenticated requests to
     pass through (for public endpoints). Sets `Option<AuthUser>` in extensions.

7. **Implement `rbac.rs`.**
   - `RequireRole` axum extractor — checks `AuthUser.roles` for the required role, returns
     403 `Forbidden` if the role is missing.
   - `resolve_user_scopes` — query `user_roles` and scope tables to determine all scopes
     the user has access to (own user scope + group scopes).

8. **Implement `service.rs`.**
   - `register(email, username, password) -> Result<UserId>` — validate, hash, insert user
     with `Pending` status.
   - `login(email, password) -> Result<(AuthUser, SessionToken)>` — authenticate, check
     status, create session.
   - `logout(session_id) -> Result<()>` — delete session.
   - `approve_user(email) -> Result<()>` — set status to `Active`.
   - `disable_user(email) -> Result<()>` — set status to `Disabled`.
   - `list_users(filter) -> Result<Vec<UserSummary>>` — list users with optional status filter.

9. **Write unit tests.**
   - Registration: password too short rejected, valid password accepted.
   - Login: correct credentials succeed, wrong password fails, pending account rejected,
     disabled account rejected.
   - Session: creation returns valid token, validation succeeds with correct token, expired
     session rejected, deletion invalidates session.
   - RBAC: user with required role passes, user without role gets 403.

10. **Write integration tests (require running Postgres).**
    Full end-to-end flow: register -> approve -> login -> validate session -> logout.
    Verify session is invalid after logout. Verify expired session cleanup removes old entries.

---

## Acceptance Criteria

- All unit tests pass.
- Integration tests pass against a real PostgreSQL instance.
- `cargo clippy -p sober-auth -- -D warnings` is clean.
- No plaintext session tokens stored anywhere (only SHA-256 hashes in DB and moka cache).
- Session cookie has `HttpOnly`, `Secure`, and `SameSite=Lax` flags set.
- Expired sessions are rejected even if still present in the database.
