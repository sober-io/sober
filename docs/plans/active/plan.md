# 006 — sober-auth: Implementation Plan

**Date:** 2026-03-06
**Updated:** 2026-03-08

---

## Steps

1. **Add dependencies to Cargo.toml.**
   Add `sober-crypto`, `sober-core` as path dependencies. Add `tower`, `sha2`, `hex`
   as workspace dependencies. Add `rand_core`, `axum`, `axum-core`, `http`, `chrono`,
   `tokio`, `serde`, `thiserror`, `tracing` from workspace.

2. **Create module structure.**
   - `src/lib.rs` — public API, re-exports.
   - `src/error.rs` — `AuthError` enum.
   - `src/token.rs` — session token generation and hashing.
   - `src/extractor.rs` — `AuthUser` and `RequireAdmin` axum extractors.
   - `src/service.rs` — high-level `AuthService` with generic repo parameters.
   - `src/middleware.rs` — auth middleware (tower Layer).

3. **Implement `error.rs`.**
   Define `AuthError` enum with variants: `PasswordTooShort`, `InvalidCredentials`,
   `AccountNotActive`, `SessionNotFound`, `InsufficientRole(RoleKind)`.
   Implement `From<AuthError> for AppError`.

4. **Implement `token.rs`.**
   - `generate_session_token()` — 256-bit random token via OsRng, returns `(raw_hex, sha256_hex)`.
   - `hash_token(raw_hex)` — SHA-256 hash of hex-decoded token bytes.

5. **Implement `extractor.rs`.**
   - `AuthUser` struct with `user_id: UserId` and `roles: Vec<RoleKind>`.
   - `AuthUser` implements `FromRequestParts` — extracts from request extensions.
   - `RequireAdmin(AuthUser)` — wraps `AuthUser`, checks for `RoleKind::Admin`, returns 403 if missing.

6. **Add `RoleRepo` trait to `sober-core`.**
   `get_roles_for_user(user_id) -> Vec<RoleKind>`. Add `RoleKind` enum to `sober-core`
   with `User`, `Admin`, and `Custom(String)` variants. Implement `PgRoleRepo` in `sober-db`.

7. **Implement `service.rs`.**
   - `AuthService<U: UserRepo, S: SessionRepo, R: RoleRepo>` — generic over repos
     (required because RPITIT traits are not dyn-compatible).
   - `register(email, username, password) -> Result<User>` — validate password length,
     hash with Argon2id via `spawn_blocking`, create user with `Pending` status and `[RoleKind::User]` role(s).
   - `login(email, password) -> Result<(String, User)>` — authenticate, **always run Argon2
     before checking status** (prevents timing oracle), create session, return raw token.
   - `validate_session(raw_token) -> Result<AuthUser>` — hash token, look up session, load roles.
   - `logout(raw_token) -> Result<()>` — hash token, delete session.
   - `approve_user(user_id) -> Result<()>` — set status to `Active`.
   - `disable_user(user_id) -> Result<()>` — set status to `Disabled`.

8. **Implement `middleware.rs`.**
   - `AuthLayer<U, S, R>` — tower `Layer` backed by `Arc<AuthService<U, S, R>>`.
   - `AuthMiddleware<Svc, U, S, R>` — tower `Service` that extracts token synchronously
     (before async boundary), validates session, inserts `AuthUser` into extensions.
   - Token extraction: `Authorization: Bearer` header first, `sober_session` cookie fallback.
   - `extract_bearer_token()` — extracts token from `Authorization: Bearer <token>` header.
   - `extract_cookie()` — parses `Cookie` header, finds `sober_session`, strips quoted-string wrapping.
   - `cookie_name()` — returns the session cookie name constant.

9. **Write unit tests.**
   - Error: all variants map to correct `AppError` variants and HTTP status codes.
   - Token: generates 64-char hex tokens, hash matches, rejects invalid hex.
   - Extractor: `has_role` returns correct results for matching/missing roles.

10. **Write integration tests (require running Postgres).**
    Full end-to-end flow: register -> approve -> login -> validate session -> logout.
    Verify session is invalid after logout.

---

## Acceptance Criteria

- All unit tests pass.
- Integration tests pass against a real PostgreSQL instance.
- `cargo clippy -p sober-auth -- -D warnings` is clean.
- No plaintext session tokens stored anywhere (only SHA-256 hashes in DB).
- Argon2 verification always runs before status check in login (no timing oracle).
- Expired sessions are rejected at the SQL query level.
