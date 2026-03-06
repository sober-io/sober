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
| LLM | `LlmConfig` | `LLM_BASE_URL`, `LLM_API_KEY`, `LLM_MODEL`, `LLM_MAX_TOKENS`, `EMBEDDING_MODEL` |
| Server | `ServerConfig` | `HOST`, `PORT` |
| Auth | `AuthConfig` | `SESSION_SECRET`, `SESSION_TTL_SECONDS` |
| SearXNG | `SearxngConfig` | `SEARXNG_URL` |
| Admin | `AdminConfig` | `ADMIN_SOCKET_PATH` (default: `/run/sober/admin.sock`) |
| Memory | `MemoryConfig` | `MEMORY_DECAY_HALF_LIFE_DAYS` (default: 30), `MEMORY_RETRIEVAL_BOOST` (default: 0.2), `MEMORY_PRUNE_THRESHOLD` (default: 0.1) |

### Domain Primitives --- ID Newtypes

All entity IDs are UUIDv7 (time-ordered) newtypes. A macro generates the boilerplate for
each type to keep the code DRY.

Types: `UserId`, `ScopeId`, `ConversationId`, `MessageId`, `SessionId`, `RoleId`, `McpServerId`, `WorkspaceId`.

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

### Tool Trait

A common trait for all tool implementations, defined in `sober-core` so that downstream
crates (`sober-mcp`, `sober-plugin`, `sober-agent`) share a single interface.

```rust
pub struct ToolMetadata {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value, // JSON Schema describing accepted input
}

pub struct ToolOutput {
    pub content: serde_json::Value,
    pub is_error: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Tool not found: {0}")]
    NotFound(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn metadata(&self) -> ToolMetadata;
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError>;
}
```

`ToolMetadata`, `ToolOutput`, and `ToolError` are concrete types; `Tool` is the trait that
any tool (MCP, plugin, built-in) must implement.

### Access Mask / Caller Context

Describes who triggered an operation and what they are allowed to access. Used by
`sober-mind` during prompt assembly and by `sober-agent` for authorization checks.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TriggerKind {
    Human,
    Scheduler,
    Replica,
    Admin,
}

#[derive(Debug, Clone)]
pub struct CallerContext {
    pub user_id: Option<UserId>,
    pub trigger: TriggerKind,
    pub permissions: Vec<Permission>,
    pub scope_grants: Vec<ScopeId>,
}
```

`TriggerKind` determines the access tier (see ARCHITECTURE.md access mask table).
`CallerContext` is constructed at the entry point (API handler, scheduler tick, replica
delegation) and threaded through the call chain. The `permissions` field holds the
caller's resolved RBAC/ABAC permissions; `scope_grants` lists the scopes the caller
may read from or write to.

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

### Telemetry Setup

`init_telemetry(config: &AppConfig)` function that configures the full observability stack:

1. **Tracing subscriber** (`tracing-subscriber`):
   - **Development:** Pretty-printed, human-readable format with ANSI colors.
   - **Production:** JSON-structured format for log aggregation.
   - Log level configurable via `RUST_LOG` env var (defaults to `info`).
   - The environment is determined by a `SOBER_ENV` variable (`development` vs `production`).

2. **OpenTelemetry trace export** (`tracing-opentelemetry` + `opentelemetry-otlp`):
   - Exports spans to Grafana Tempo (or any OTLP-compatible backend).
   - Configurable via `OTEL_EXPORTER_OTLP_ENDPOINT`. Disabled if unset.
   - Service name set via `OTEL_SERVICE_NAME` (auto-set per binary if absent).

3. **Prometheus metrics recorder** (`metrics` + `metrics-exporter-prometheus`):
   - Always active (in-memory registry). Exposes a `/metrics` handler.
   - `MetricsEndpoint` axum handler exported for services to mount.

All backends (Prometheus, Tempo) are optional consumers. The app operates
normally when they are not running. Disabling OTEL export is a config change.

Standard label constants exported: `service`, `method`, `status`, `crate`.

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

### Caching Strategy

v1 uses **moka** for in-memory caching (no Redis). Redis may be introduced post-v1 for
distributed deployments, but all v1 services run on a single node and moka provides
sufficient TTL-based caching with minimal operational overhead.

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
| `async-trait` | Async trait support (for `Tool` trait) |
| `moka` (with `future`) | In-memory TTL cache (v1 caching layer) |
| `tracing-opentelemetry` | Bridge tracing spans to OpenTelemetry |
| `opentelemetry` | OTEL API |
| `opentelemetry-otlp` | OTLP exporter for Tempo |
| `opentelemetry_sdk` | OTEL SDK runtime |
| `metrics` | In-process metric registry |
| `metrics-exporter-prometheus` | Prometheus `/metrics` endpoint |
