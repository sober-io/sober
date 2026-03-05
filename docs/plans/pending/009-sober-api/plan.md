# 009 — sober-api: Implementation Plan

**Date:** 2026-03-06
**Design:** [design.md](./design.md)

## Steps

1. **Add dependencies to sober-api `Cargo.toml`.**
   axum (ws, json, macros), axum-extra (cookie), tokio (full), tower, tower-http
   (cors, trace, request-id), sqlx (postgres), redis, hyper, hyper-util, serde,
   serde_json, tracing, uuid. Workspace dependencies on sober-core, sober-auth,
   sober-agent.

2. **Create module structure.**
   - `src/main.rs` — entry point, startup, shutdown
   - `src/state.rs` — `AppState` struct, initialization
   - `src/routes/mod.rs` — router assembly
   - `src/routes/health.rs` — health check handler
   - `src/routes/auth.rs` — register, login, logout, me
   - `src/routes/conversations.rs` — conversation CRUD
   - `src/routes/mcp.rs` — MCP server config CRUD
   - `src/routes/ws.rs` — WebSocket upgrade and message handling
   - `src/middleware/mod.rs` — middleware re-exports
   - `src/middleware/rate_limit.rs` — Redis-backed rate limiter
   - `src/admin.rs` — Unix socket listener

3. **Implement `state.rs`.**
   AppState struct with PgPool, RedisPool, Agent, AppConfig. Constructor that
   connects to all backends and fails fast on error.

4. **Implement `routes/health.rs`.**
   `GET /health` returning `{ "data": { "status": "ok" } }`.

5. **Implement `routes/auth.rs`.**
   Thin handlers that delegate to sober-auth: register, login (sets HttpOnly
   session cookie), logout (clears cookie), me (returns current AuthUser).

6. **Implement `routes/conversations.rs`.**
   CRUD handlers: list (paginated), create, get (with messages), update title,
   delete. All scoped to the authenticated user.

7. **Implement `routes/mcp.rs`.**
   CRUD handlers for MCP server configurations. Scoped to the authenticated user.

8. **Implement `routes/ws.rs`.**
   WebSocket upgrade handler. Session validation from cookie. JSON message
   parsing (chat.message, chat.cancel). Spawn agent task per message, forward
   AgentEvent stream to WebSocket. CancellationToken for chat.cancel. Clean
   disconnect handling.

9. **Implement `middleware/rate_limit.rs`.**
   Redis-backed sliding window rate limiter as a tower Layer. Configurable
   limits per endpoint pattern. Returns 429 with Retry-After header.

10. **Implement `admin.rs`.**
    Unix socket listener using tokio UnixListener. Serves a minimal router
    (health check only for v1). Binds only when ADMIN_SOCKET_PATH is configured.

11. **Implement `main.rs`.**
    Startup sequence (config, tracing, connections, agent, router, middleware
    stack, admin socket, TCP listener). Graceful shutdown on SIGTERM/SIGINT
    with configurable timeout.

12. **Write integration tests: HTTP endpoints.**
    Using `tower::ServiceExt::oneshot`:
    - Health check returns 200.
    - Auth flow: register, login (receives cookie), me (returns user), logout.
    - Conversation CRUD: create, list, get, update title, delete.
    - Proper error envelopes on 404, 401, 422.

13. **Write integration tests: WebSocket.**
    Connect with valid session, send chat.message, receive streamed events
    (chat.delta, chat.done). Test chat.cancel stops the stream. Test invalid
    session is rejected before upgrade.

14. **Lint and test.**
    Run `cargo clippy -p sober-api -- -D warnings` and `cargo test -p sober-api`.

## Acceptance Criteria

- Health check returns 200 with `{ "data": { "status": "ok" } }`.
- Auth flow works end-to-end (register, login with session cookie, me returns
  user, logout clears session).
- Conversation CRUD works with proper scope isolation (users cannot access
  other users' conversations).
- WebSocket chat streams agent responses as typed JSON events.
- Rate limiting returns 429 with Retry-After header when limits are exceeded.
- Admin socket binds at configured path and responds to health check.
- CORS headers present on all responses.
- All handlers return the standard error envelope on failure.
- Graceful shutdown completes within the configured timeout.
- `cargo clippy -p sober-api -- -D warnings` produces no warnings.
- `cargo test -p sober-api` passes.
