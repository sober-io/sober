# 003 --- sober-core: Implementation Plan

**Date:** 2026-03-06
**Status:** Pending
**Depends on:** 002 (workspace bootstrap)

---

## Steps

### 1. Cargo.toml

Add all dependencies to `backend/crates/sober-core/Cargo.toml`. Reference workspace-level
dependency declarations where possible. Enable the required features for each crate:

- `uuid = { version = "1", features = ["v7", "serde"] }`
- `sqlx = { version = "0.8", features = ["postgres", "runtime-tokio", "tls-rustls", "uuid"] }`
- `serde = { version = "1", features = ["derive"] }`
- `chrono = { version = "0.4", features = ["serde"] }`
- `tracing-subscriber = { version = "0.3", features = ["env-filter", "json", "fmt"] }`
- Remaining crates at latest stable versions.

### 2. Module Structure

Create the following file layout:

```
backend/crates/sober-core/src/
  lib.rs          # Public API, module declarations, re-exports
  error.rs        # AppError enum, IntoResponse impl
  config.rs       # AppConfig and section structs, load_from_env()
  telemetry.rs    # init_telemetry() function (tracing + OTEL + Prometheus)
  admin.rs        # AdminCommand, AdminResponse enums
  types/
    mod.rs        # Re-exports from submodules
    ids.rs        # UUIDv7 newtype macro and ID types
    enums.rs      # ScopeKind, UserStatus, MessageRole, TriggerKind
    api.rs        # ApiResponse<T>, ApiErrorBody
    tool.rs       # Tool trait, ToolMetadata, ToolOutput, ToolError
    access.rs     # CallerContext, Permission (placeholder)
```

### 3. Implement `error.rs`

- Define `AppError` enum with all six variants.
- Derive `Debug` and `thiserror::Error`.
- Implement `axum_core::response::IntoResponse`:
  - Map each variant to its HTTP status code.
  - Serialize the response body as `{ "error": { "code": "...", "message": "..." } }`.
- Implement `From<anyhow::Error>` via the `#[from]` attribute on the `Internal` variant.

### 4. Implement `config.rs`

- Define `AppConfig` with nested section structs: `DatabaseConfig`, `QdrantConfig`,
  `LlmConfig`, `ServerConfig`, `AuthConfig`, `SearxngConfig`, `AdminConfig`.
  (No `RedisConfig` in v1 --- use `moka` for in-memory caching instead.)
- Implement `AppConfig::load_from_env() -> Result<Self, AppError>`:
  - Call `dotenvy::dotenv().ok()` (non-fatal if `.env` is absent).
  - Read each required variable with `std::env::var`, mapping missing vars to
    `AppError::Validation` with a descriptive message.
  - Parse numeric values (port, max connections, TTL) with clear error messages.
  - Apply defaults where documented (e.g., `ADMIN_SOCKET_PATH`).
- All config fields are owned (`String`, `u16`, `u32`, etc.) --- no lifetimes.

### 5. Implement `types/ids.rs`

- Define a `define_id!` macro that generates a newtype wrapper around `Uuid`:
  - `pub struct $Name(Uuid);`
  - Derives: `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `Serialize`, `Deserialize`.
  - `sqlx::Type` with `transparent` representation.
  - `impl $Name { fn new(), fn from_uuid(), fn as_uuid() }`
  - `impl Display` delegating to inner UUID.
- Invoke the macro for: `UserId`, `ScopeId`, `ConversationId`, `MessageId`, `SessionId`,
  `RoleId`, `McpServerId`, `WorkspaceId`.

### 6. Implement `types/enums.rs`

- `ScopeKind` with variants: `System`, `User`, `Group`, `Session`.
- `UserStatus` with variants: `Pending`, `Active`, `Disabled`.
- `MessageRole` with variants: `User`, `Assistant`, `System`, `Tool`.
- All derive: `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `Serialize`,
  `Deserialize`, `sqlx::Type`.
- Use `#[sqlx(type_name = "...", rename_all = "lowercase")]` to match PostgreSQL enums.
- Use `#[serde(rename_all = "lowercase")]` for consistent JSON serialization.

### 7. Implement `types/api.rs`

- `ApiResponse<T: Serialize>` --- wraps a `data: T` field.
  - Implement `IntoResponse` to serialize as `{ "data": ... }` with status 200.
- `ApiErrorBody` --- struct with `code: String` and `message: String`.
  - Used internally by `AppError::into_response`.

### 8. Implement `types/tool.rs`

- Define `ToolMetadata` struct: `name: String`, `description: String`, `input_schema: serde_json::Value`.
- Define `ToolOutput` struct: `content: serde_json::Value`, `is_error: bool`.
- Define `ToolError` enum with `thiserror`: `NotFound(String)`, `InvalidInput(String)`,
  `ExecutionFailed(String)`, `Internal(#[from] anyhow::Error)`.
- Define `Tool` trait (using `async_trait`): `fn metadata(&self) -> ToolMetadata`,
  `async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError>`.
- All types derive appropriate traits (`Debug`, `Clone`, `Serialize`, `Deserialize` where applicable).

### 9. Implement `types/access.rs`

- Define `TriggerKind` enum: `Human`, `Scheduler`, `Replica`, `Admin`.
  - Derive: `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `Serialize`, `Deserialize`.
- Define `CallerContext` struct:
  - `user_id: Option<UserId>`
  - `trigger: TriggerKind`
  - `permissions: Vec<Permission>`
  - `scope_grants: Vec<ScopeId>`
- `CallerContext` derives `Debug`, `Clone`.
- Note: `Permission` enum may be a placeholder in v1, expanded by `sober-auth` later.

### 10. Implement `admin.rs`

- `AdminCommand` enum (serde-tagged):
  - `Ping`, `AgentStatus`, `TaskQueueStatus`, `PruneMemory { scope_id: Option<ScopeId> }`,
    `ReloadConfig`, `Shutdown { graceful: bool }`.
- `AdminResponse` enum (serde-tagged):
  - `Pong`, `Status(serde_json::Value)`, `Ok`, `Error(String)`.
- Both derive `Debug`, `Clone`, `Serialize`, `Deserialize`.
- Use `#[serde(tag = "type", rename_all = "snake_case")]` for clean JSON representation.

### 11. Implement `telemetry.rs`

- `pub fn init_telemetry(config: &AppConfig)`:
  - Read `SOBER_ENV` to determine format (pretty vs JSON).
  - Configure `EnvFilter` from `RUST_LOG` with a default of `info`.
  - Build and install the global tracing subscriber.
  - If `OTEL_EXPORTER_OTLP_ENDPOINT` is set, add OpenTelemetry trace export layer
    (via `tracing-opentelemetry` + `opentelemetry-otlp`) pointing at Tempo.
  - Install Prometheus metrics recorder (`metrics-exporter-prometheus`). Always active.
  - Export `MetricsEndpoint` axum handler for `/metrics` endpoint.
- Keep this function idempotent-safe (guard against double-init panics with `try_init`).
- Graceful degradation: app works fine if Tempo/Prometheus are not running.

### 12. Wire up `lib.rs`

- Declare all modules.
- Re-export key types at the crate root for ergonomic imports:
  - `pub use error::AppError;`
  - `pub use config::AppConfig;`
  - `pub use types::*;`
  - `pub use admin::{AdminCommand, AdminResponse};`
  - `pub use telemetry::init_telemetry;`
  - `pub use uuid::Uuid;`
  - `pub use chrono::{DateTime, Utc};`

### 13. Tests

Write unit tests covering:

- **Config parsing:** Set env vars in test scope (use `std::env::set_var` or a helper),
  verify `AppConfig::load_from_env()` succeeds. Verify missing required vars produce
  `AppError::Validation`.
- **Error mapping:** Construct each `AppError` variant, call `into_response()`, assert the
  correct HTTP status code and JSON body structure.
- **ID generation:** Call `UserId::new()` twice, verify both are valid UUIDv7 and distinct.
  Verify `Display` output matches UUID format.
- **Enum serialization:** Roundtrip each enum through `serde_json` (serialize then
  deserialize), verify values match. Verify the serialized form is lowercase.
- **TriggerKind serialization:** Roundtrip all variants through `serde_json`.
- **CallerContext construction:** Build a `CallerContext` for each `TriggerKind`, verify fields.
- **ToolError variants:** Construct each variant, verify `Display` output.

### 14. Verification

Run the following and fix any issues:

```bash
cargo clippy -p sober-core -- -D warnings
cargo test -p sober-core
cargo doc -p sober-core --no-deps
```

---

## Acceptance Criteria

- [ ] All types compile and are importable from downstream crates via `use sober_core::*`.
- [ ] Unit tests pass for config, errors, ID generation, and enum serialization.
- [ ] `cargo clippy -p sober-core -- -D warnings` reports zero warnings.
- [ ] `cargo doc -p sober-core --no-deps` generates documentation without warnings.
- [ ] No `.unwrap()` in library code (only in tests).
- [ ] All public items have doc comments.
