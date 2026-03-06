# 012 — sober-api

**Date:** 2026-03-06

## Overview

Binary crate that runs the axum HTTP server. Thin handlers — all business logic
lives in service crates (sober-auth, sober-agent, sober-memory, etc.). The API
gateway is responsible for routing, middleware, WebSocket management, and the
admin Unix socket.

## Application State

```rust
pub struct AppState {
    pub db: PgPool,
    pub redis: RedisPool,
    pub agent_client: AgentClient,
    pub config: AppConfig,
}
```

`AgentClient` is a tonic gRPC client (`AgentServiceClient`) that connects to the
agent's Unix domain socket at `/run/sober/agent.sock`. The API server does NOT
instantiate an `Agent` directly — it communicates with the agent process via gRPC.

Wrapped in `Arc<AppState>` and passed via axum `State`.

## Router Structure

```
/api/v1/
  /health                    GET     — health check (no auth)
  /auth/register             POST    — register new user
  /auth/login                POST    — login, returns session cookie
  /auth/logout               POST    — logout (requires auth)
  /auth/me                   GET     — current user info (requires auth)
  /conversations             GET     — list conversations (requires auth, scoped by user_id)
  /conversations             POST    — create conversation (requires auth, scoped by user_id, no scope_id)
  /conversations/:id         GET     — get conversation with messages (requires auth)
  /conversations/:id         PATCH   — update title (requires auth)
  /conversations/:id         DELETE  — delete conversation (requires auth)
  /mcp/servers               GET     — list MCP server configs (requires auth)
  /mcp/servers               POST    — add MCP server config (requires auth)
  /mcp/servers/:id           PATCH   — update MCP server config (requires auth)
  /mcp/servers/:id           DELETE  — remove MCP server config (requires auth)
  /ws                        GET     — WebSocket upgrade (requires auth via cookie)
```

## Middleware Stack

Tower layers, applied in order:

1. **Request ID** — generate UUID, attach to tracing span.
2. **Tracing** — tower-http trace layer; logs method, path, status, duration.
3. **CORS** — configurable allowed origins.
4. **Rate limiting** — Redis-backed, per-IP for public endpoints, per-user for authenticated.
5. **Auth** — from sober-auth; extracts session cookie, validates, attaches `AuthUser`.

## WebSocket Chat

- Client connects to `/api/v1/ws` with session cookie.
- Server validates session, upgrades to WebSocket.
- Single WebSocket endpoint at `/api/v1/ws` (no path parameter). All messages
  include `conversation_id` in the payload for multiplexing.
- **Client-to-server messages (ClientWsMessage):**
  - `{ "type": "chat.message", "conversation_id": "...", "content": "..." }`
  - `{ "type": "chat.cancel", "conversation_id": "..." }`
- **Server-to-client messages (ServerWsMessage):**
  - `{ "type": "chat.delta", "conversation_id": "...", "content": "..." }`
  - `{ "type": "chat.tool_use", "conversation_id": "...", "tool_call": {...} }`
  - `{ "type": "chat.tool_result", "conversation_id": "...", "tool_call_id": "...", "output": "..." }`
  - `{ "type": "chat.done", "conversation_id": "...", "message_id": "..." }`
  - `{ "type": "chat.error", "conversation_id": "...", "error": "..." }`
- One WebSocket connection per user (multiplexed by `conversation_id` in every message).
- On `chat.message`: spawn tokio task, call agent via gRPC streaming
  (`agent_client.handle_message`), forward AgentEvent stream to WebSocket
  with the originating `conversation_id` attached to each event.
- On `chat.cancel`: signal the agent task to cancel via `tokio::CancellationToken`.
- Handle disconnects gracefully (drop agent task).

## Admin Socket

- Unix domain socket at `ADMIN_SOCKET_PATH` (opt-in via config).
- Same axum router but only the `/health` endpoint exposed for v1.
- Access controlled by filesystem permissions (no auth over socket).
- Uses hyper's Unix socket listener (via tokio `UnixListener`).

## Rate Limiting

Redis-backed sliding window counter.

| Endpoint               | Limit         | Scope  |
|------------------------|---------------|--------|
| `POST /auth/login`     | 5/min         | per-IP |
| `POST /auth/register`  | 3/hr          | per-IP |
| `chat.message` (WS)    | 20/min        | per-user |
| All other              | 60/min        | per-user |

Returns `429 Too Many Requests` with `Retry-After` header when exceeded.

## Request/Response Conventions

- All responses use envelope format:
  - Success: `{ "data": ... }`
  - Error: `{ "error": { "code": "...", "message": "..." } }`
- Content-Type: `application/json`
- Errors handled by `AppError`'s `IntoResponse` impl from sober-core.

## Startup Sequence

1. Load config (fail fast on missing or invalid values).
2. Init tracing subscriber.
3. Connect to PostgreSQL (`sqlx::PgPool`).
4. Connect to Redis.
5. Connect to agent gRPC service via UDS (`AgentServiceClient` connecting to `/run/sober/agent.sock`).
6. Build `AppState` with `AgentClient` (not an `Agent` instance).
7. Build router and apply middleware stack.
8. Optionally bind admin Unix socket.
9. Bind TCP listener, serve.

## Graceful Shutdown

- Listen for `SIGTERM` / `SIGINT`.
- Stop accepting new connections.
- Wait for in-flight requests to complete (with configurable timeout).
- Close database connections.
- Shutdown MCP client connections.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `sober-core` | Shared types, AppError, config |
| `sober-auth` | Authentication middleware and handlers |
| `tonic` | gRPC client for agent service |
| `prost` | Protocol Buffers types |
| `axum` | HTTP framework (features: ws, json, macros) |
| `axum-extra` | Cookie extraction |
| `tokio` | Async runtime (full features) |
| `tower` | Middleware composition |
| `tower-http` | CORS, tracing, request-id layers |
| `sqlx` | PostgreSQL pool |
| `redis` | Async Redis client |
| `hyper` / `hyper-util` | Unix socket serving |
| `serde` / `serde_json` | Serialization |
| `tracing` | Structured logging |
| `uuid` | Request IDs, entity IDs |
