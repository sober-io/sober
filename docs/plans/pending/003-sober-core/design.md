# 003 --- sober-core

**Date:** 2026-03-06
**Status:** Pending
**Crate:** `sober-core`

---

## Overview

`sober-core` is the foundation crate of the Sober workspace. It contains shared types,
error handling, configuration, domain primitives, admin protocol types, and tracing setup.
Every other crate in the workspace depends on `sober-core`. It has no dependencies on any
sibling crate.

---

## Design Decisions

### Error Handling

Central `AppError` enum using `thiserror`, with an `IntoResponse` implementation for axum
that maps each variant to an appropriate HTTP status code.

Variants:

| Variant | HTTP Status | Description |
|---------|-------------|-------------|
| `NotFound(String)` | 404 | Resource not found |
| `Validation(String)` | 400 | Invalid input |
| `Unauthorized` | 401 | Missing or invalid credentials |
| `Forbidden` | 403 | Insufficient permissions |
| `Conflict(String)` | 409 | Duplicate or conflicting state |
| `Internal(anyhow::Error)` | 500 | Unexpected internal error (wraps `anyhow`) |

The `Internal` variant uses `#[from] anyhow::Error` so that any error can be converted
via `?` with an `.context()` call. Library-level code within sober-core itself uses
`thiserror` exclusively; `anyhow` is only used at the boundary for wrapping opaque errors.

### Configuration

`AppConfig` struct loaded from environment variables via `dotenvy` and manual parsing.
No configuration framework (e.g., `config` crate) --- direct `std::env::var` calls with
explicit error messages on missing or malformed values. The application fails fast at
startup if any required variable is absent.

Config sections:

| Section | Struct | Key variables |
|---------|--------|---------------|
| Database | `DatabaseConfig` | `DATABASE_URL`, `DATABASE_MAX_CONNECTIONS` |
| Qdrant | `QdrantConfig` | `QDRANT_URL`, `QDRANT_API_KEY` |
| Redis | `RedisConfig` | `REDIS_URL` |
| LLM | `LlmConfig` | `LLM_PROVIDER`, `LLM_API_KEY`, `LLM_MODEL` |
| Server | `ServerConfig` | `HOST`, `PORT` |
| Auth | `AuthConfig` | `SESSION_SECRET`, `SESSION_TTL_SECONDS` |
| SearXNG | `SearxngConfig` | `SEARXNG_URL` |
| Admin | `AdminConfig` | `ADMIN_SOCKET_PATH` (default: `/run/sober/admin.sock`) |

### Domain Primitives --- ID Newtypes

All entity IDs are UUIDv7 (time-ordered) newtypes. A macro generates the boilerplate for
each type to keep the code DRY.

Types: `UserId`, `ScopeId`, `ConversationId`, `MessageId`, `SessionId`, `RoleId`, `McpServerId`.

Each newtype derives:
- `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`
- `Serialize`, `Deserialize` (serde)
- `sqlx::Type` (transparent, maps directly to PostgreSQL UUID)

Each newtype provides:
- `fn new() -> Self` --- generates a new UUIDv7
- `fn from_uuid(uuid: Uuid) -> Self` --- wraps an existing UUID
- `fn as_uuid(&self) -> &Uuid` --- borrows the inner UUID
- `Display` implementation that delegates to the inner UUID

The `uuid` crate v1 with the `v7` feature is used for generation.

### Domain Enums

Enums that map directly to PostgreSQL custom types via sqlx:

**`ScopeKind`** --- `system`, `user`, `group`, `session`
Maps to the `scope_kind` PostgreSQL enum. Determines the isolation level of a memory scope.

**`UserStatus`** --- `pending`, `active`, `disabled`
Maps to the `user_status` PostgreSQL enum. Controls account lifecycle.

**`MessageRole`** --- `user`, `assistant`, `system`, `tool`
Maps to the `message_role` PostgreSQL enum. Identifies the author type of a message.

All enums derive: `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `Serialize`,
`Deserialize`, `sqlx::Type`. The sqlx type name and rename_all are set to match the
PostgreSQL enum (lowercase snake_case).

### Admin Protocol Types

Types for communication between the CLI tools (`soberctl`) and the API server over a Unix
domain socket. These live in `sober-core` so that both `sober-cli` and `sober-api` can
depend on them without `sober-cli` depending on `sober-api`.

**`AdminCommand`** --- enum of commands that `soberctl` can send:
- `Ping` --- health check
- `AgentStatus` --- query running agent state
- `TaskQueueStatus` --- query task queue depth and state
- `PruneMemory { scope_id: Option<ScopeId> }` --- trigger memory pruning
- `ReloadConfig` --- hot-reload configuration
- `Shutdown { graceful: bool }` --- initiate shutdown

**`AdminResponse`** --- enum of responses:
- `Pong` --- response to Ping
- `Status(serde_json::Value)` --- JSON payload with status info
- `Ok` --- command accepted
- `Error(String)` --- command failed with reason

Both are `Serialize` + `Deserialize` for JSON encoding over the socket.

### Tracing Setup

`init_tracing(config: &AppConfig)` function that configures the `tracing-subscriber`:
- **Development:** Pretty-printed, human-readable format with ANSI colors.
- **Production:** JSON-structured format for log aggregation.
- Log level configurable via `RUST_LOG` env var (defaults to `info`).
- The environment is determined by a `SOBER_ENV` variable (`development` vs `production`).

### Response Envelope Types

Standardized API response wrappers:

```rust
/// Successful response: { "data": T }
pub struct ApiResponse<T: Serialize> {
    pub data: T,
}

/// Error body: { "error": { "code": "...", "message": "..." } }
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
}
```

`ApiResponse<T>` implements `IntoResponse` for axum, serializing to
`{ "data": ... }` with a 200 status. `AppError::into_response` produces the
error envelope format.

### Re-exports

`sober-core` re-exports commonly used types so downstream crates do not need to
declare direct dependencies on utility crates:

- `pub use uuid::Uuid;`
- `pub use chrono::{DateTime, Utc};`

---

## Dependencies

| Crate | Purpose |
|-------|---------|
| `thiserror` | Error derive macros |
| `anyhow` | Opaque error wrapping (for `AppError::Internal`) |
| `serde` (with `derive`) | Serialization |
| `serde_json` | JSON support |
| `uuid` (with `v7`, `serde`) | UUIDv7 generation and serialization |
| `sqlx` (with `postgres`, `runtime-tokio`, `tls-rustls`, `uuid`) | PostgreSQL type mapping |
| `tracing` | Instrumentation API |
| `tracing-subscriber` (with `env-filter`, `json`, `fmt`) | Subscriber configuration |
| `dotenvy` | Environment variable loading |
| `chrono` (with `serde`) | Date/time types |
| `axum-core` | `IntoResponse` trait (minimal axum dependency) |
| `axum` (with `json`) | `Json` extractor for response serialization |
| `http` | `StatusCode` type |
