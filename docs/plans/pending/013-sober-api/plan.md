# 013 — sober-api: Implementation Plan

**Date:** 2026-03-06
**Design:** [design.md](./design.md)

## Steps

1. **Add dependencies to sober-api `Cargo.toml`.**
   axum (ws, json, macros), axum-extra (cookie), tokio (full), tower, tower-http
   (cors, trace, request-id), sqlx (postgres), moka, hyper, hyper-util, tonic
   (gRPC client), prost, serde, serde_json, tracing, uuid. Workspace dependencies
   on sober-core, sober-auth. Note: sober-agent is NOT a crate dependency — the API
   communicates with the agent via gRPC client using shared proto definitions.

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
   - `src/middleware/rate_limit.rs` — moka-backed rate limiter
   - `src/admin.rs` — Unix socket listener

3. **Implement `state.rs`.**
   AppState struct with PgPool, AgentClient (tonic gRPC client), AppConfig.
   Constructor connects to PostgreSQL and the agent gRPC service at
   `/run/sober/agent.sock`. Fails fast on error. No `Agent` struct instantiation —
   the agent runs as a separate process.

4. **Implement `routes/health.rs`.**
   `GET /health` returning `{ "data": { "status": "ok" } }`.

5. **Implement `routes/auth.rs`.**
   Thin handlers that delegate to sober-auth: register, login (sets HttpOnly
   session cookie), logout (clears cookie), me (returns current AuthUser).

6. **Implement `routes/conversations.rs`.**
   CRUD handlers: list (paginated), create, get (with messages), update title,
   delete. All scoped to the authenticated user via user_id. No scope_id on
   conversation creation — conversations are scoped by user_id only.

7. **Implement `routes/mcp.rs`.**
   CRUD handlers for MCP server configurations. Scoped to the authenticated user.

8. **Implement `routes/ws.rs`.**
   WebSocket upgrade handler at `/api/v1/ws` (single endpoint, no path param).
   Session validation from cookie. JSON message parsing — all messages include
   `conversation_id` in payload (ClientWsMessage types: chat.message, chat.cancel).
   Spawn agent task per chat.message, call agent via gRPC streaming
   (`agent_client.handle_message`), forward AgentEvent stream to WebSocket with
   `conversation_id` attached to each ServerWsMessage. CancellationToken for
   chat.cancel. Track active conversations per connection. Clean disconnect handling.

9. **Implement `middleware/rate_limit.rs`.**
   moka-backed sliding window rate limiter as a tower Layer (in-memory, no Redis).
   Configurable limits per endpoint pattern. Returns 429 with Retry-After header.
   Upgrade path to Redis documented for horizontal scaling.

10. **Implement `admin.rs`.**
    Unix socket listener using tokio UnixListener. Serves a minimal router
    (health check only for v1). Binds only when ADMIN_SOCKET_PATH is configured.

11. **Implement `main.rs`.**
    Startup sequence: config, tracing, connect to PostgreSQL, agent gRPC
    service (via UDS). Build AppState with AgentClient. Assemble router +
    middleware stack. Optionally bind admin socket. Bind TCP listener. Graceful
    shutdown on SIGTERM/SIGINT with configurable timeout.

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
