# #045: sober-gateway Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `sober-gateway` binary and supporting infrastructure to bridge external messaging platforms (starting with Discord) to Sõber conversations.

**Architecture:** New leaf binary crate (`sober-gateway`) that connects to platforms via SDK libraries, routes messages to `sober-agent` via gRPC/UDS, and delivers agent responses back to platforms. Admin CRUD routes live in `sober-api` with a `GatewayAdminService`. Frontend settings pages for platform/mapping management.

**Tech Stack:** Rust (tonic gRPC, sqlx, serenity for Discord), Svelte 5, PostgreSQL, protobuf

---

## File Structure

### New crate: `backend/crates/sober-gateway/`

| File | Responsibility |
|------|---------------|
| `Cargo.toml` | Crate manifest with bin + lib |
| `build.rs` | Compile gateway + agent proto files |
| `src/main.rs` | Binary entrypoint: config, DB, gRPC client/server, event loop |
| `src/lib.rs` | Re-exports for the library |
| `src/error.rs` | `GatewayError` enum |
| `src/types.rs` | `GatewayEvent`, `PlatformMessage`, `MessageFormat`, `Attachment`, `ExternalChannel`, `PlatformConfig` |
| `src/bridge.rs` | `PlatformBridge` trait + `PlatformBridgeRegistry` |
| `src/service.rs` | `GatewayService` — inbound routing, outbound delivery, mapping lookups |
| `src/grpc.rs` | gRPC server implementation (`ListChannels`, `Reload`, `Status`, `Health`) |
| `src/outbound.rs` | Response buffering and flush logic |
| `src/discord/mod.rs` | Discord `PlatformBridge` implementation (serenity) |
| `src/discord/handler.rs` | Serenity event handler that emits `GatewayEvent`s |
| `metrics.toml` | Metric declarations |

### Modifications to existing crates

| File | Change |
|------|--------|
| `backend/crates/sober-core/src/types/ids.rs` | Add `PlatformId`, `MappingId`, `UserMappingId` |
| `backend/crates/sober-core/src/types/domain.rs` | Add gateway domain types |
| `backend/crates/sober-core/src/types/enums.rs` | Add `PlatformType` enum |
| `backend/crates/sober-core/src/types/input.rs` | Add gateway input types |
| `backend/crates/sober-core/src/types/repo.rs` | Add `GatewayPlatformRepo`, `GatewayMappingRepo`, `GatewayUserMappingRepo` traits |
| `backend/crates/sober-core/src/config.rs` | Add `GatewayConfig` section |
| `backend/crates/sober-db/src/repos/gateway.rs` | `PgGatewayPlatformRepo`, `PgGatewayMappingRepo`, `PgGatewayUserMappingRepo` |
| `backend/crates/sober-db/src/rows.rs` | Gateway row types |
| `backend/crates/sober-db/src/repos/mod.rs` | Export gateway module |
| `backend/crates/sober-db/src/lib.rs` | Re-export gateway repos |
| `backend/crates/sober-api/src/routes/mod.rs` | Add `gateway` route module |
| `backend/crates/sober-api/src/routes/gateway.rs` | Admin gateway routes |
| `backend/crates/sober-api/src/services/mod.rs` | Add `gateway` service module |
| `backend/crates/sober-api/src/services/gateway.rs` | `GatewayAdminService` |
| `backend/crates/sober-api/src/state.rs` | Add optional `GatewayClient`, add `GatewayAdminService` |
| `backend/proto/sober/gateway/v1/gateway.proto` | Gateway gRPC service definition |

### Migrations

| File | Purpose |
|------|---------|
| `backend/migrations/20260402000001_gateway_bot_user.sql` | Seed bridge bot user |
| `backend/migrations/20260402000002_gateway_tables.sql` | `gateway_platforms`, `gateway_channel_mappings`, `gateway_user_mappings` |

### Frontend

| File | Responsibility |
|------|---------------|
| `frontend/src/lib/types/gateway.ts` | TypeScript types |
| `frontend/src/lib/services/gateway.ts` | API client |
| `frontend/src/routes/(app)/settings/gateway/+page.svelte` | Platform list page |
| `frontend/src/routes/(app)/settings/gateway/[id]/+page.svelte` | Platform detail (mappings, users) |
| `frontend/src/routes/(app)/settings/+layout.svelte` | Add Gateway tab |

### Infrastructure

| File | Purpose |
|------|---------|
| `infra/docker/Dockerfile.gateway` | Dev Dockerfile |
| `docker-compose.yml` | Add `sober-gateway` service |
| `docker-bake.hcl` | Add CI build target |
| `infra/docker/Dockerfile.ci` | Add gateway runtime stage |

---

## Task 1: ID Newtypes and Enums

**Files:**
- Modify: `backend/crates/sober-core/src/types/ids.rs`
- Modify: `backend/crates/sober-core/src/types/enums.rs`

- [ ] **Step 1: Add gateway ID newtypes**

In `backend/crates/sober-core/src/types/ids.rs`, add after the `ConversationAttachmentId` definition:

```rust
define_id!(
    /// Unique identifier for a registered messaging platform connection.
    PlatformId
);

define_id!(
    /// Unique identifier for a channel-to-conversation mapping.
    MappingId
);

define_id!(
    /// Unique identifier for an external-user-to-Sõber-user mapping.
    UserMappingId
);
```

- [ ] **Step 2: Check if `enums.rs` exists, add `PlatformType`**

Check `backend/crates/sober-core/src/types/` for where enums are defined. Add `PlatformType`:

```rust
/// External messaging platform type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(feature = "postgres", sqlx(type_name = "text", rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum PlatformType {
    Discord,
    Telegram,
    Matrix,
    Whatsapp,
}

impl std::fmt::Display for PlatformType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discord => write!(f, "discord"),
            Self::Telegram => write!(f, "telegram"),
            Self::Matrix => write!(f, "matrix"),
            Self::Whatsapp => write!(f, "whatsapp"),
        }
    }
}
```

- [ ] **Step 3: Build and verify**

Run: `cd backend && cargo build -p sober-core -q`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```
feat(core): add gateway ID newtypes and PlatformType enum
```

---

## Task 2: Gateway Domain Types

**Files:**
- Modify: `backend/crates/sober-core/src/types/domain.rs`
- Modify: `backend/crates/sober-core/src/types/input.rs`
- Modify: `backend/crates/sober-core/src/types/mod.rs` (re-exports)

- [ ] **Step 1: Add domain types to `domain.rs`**

```rust
/// A registered external messaging platform connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayPlatform {
    pub id: PlatformId,
    pub platform_type: PlatformType,
    pub display_name: String,
    pub is_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A mapping from an external channel to a Sõber conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayChannelMapping {
    pub id: MappingId,
    pub platform_id: PlatformId,
    pub external_channel_id: String,
    pub external_channel_name: String,
    pub conversation_id: ConversationId,
    pub is_thread: bool,
    pub parent_mapping_id: Option<MappingId>,
    pub created_at: DateTime<Utc>,
}

/// A mapping from an external user to a Sõber user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayUserMapping {
    pub id: UserMappingId,
    pub platform_id: PlatformId,
    pub external_user_id: String,
    pub external_username: String,
    pub user_id: UserId,
    pub created_at: DateTime<Utc>,
}
```

- [ ] **Step 2: Add input types to `input.rs`**

```rust
/// Input for creating a new platform connection.
#[derive(Debug, Deserialize)]
pub struct CreatePlatform {
    pub platform_type: PlatformType,
    pub display_name: String,
}

/// Input for updating a platform connection.
#[derive(Debug, Deserialize)]
pub struct UpdatePlatform {
    pub display_name: Option<String>,
    pub is_enabled: Option<bool>,
}

/// Input for creating a channel mapping.
#[derive(Debug, Deserialize)]
pub struct CreateChannelMapping {
    pub external_channel_id: String,
    pub external_channel_name: String,
    pub conversation_id: ConversationId,
}

/// Input for creating a user mapping.
#[derive(Debug, Deserialize)]
pub struct CreateUserMapping {
    pub external_user_id: String,
    pub external_username: String,
    pub user_id: UserId,
}
```

- [ ] **Step 3: Add re-exports in `mod.rs`**

Ensure the new types are re-exported from `sober_core::types`.

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-core -q`
Expected: compiles

- [ ] **Step 5: Commit**

```
feat(core): add gateway domain and input types
```

---

## Task 3: Repository Traits

**Files:**
- Modify: `backend/crates/sober-core/src/types/repo.rs`

- [ ] **Step 1: Add `GatewayPlatformRepo` trait**

```rust
/// Repository for gateway platform connections.
pub trait GatewayPlatformRepo: Send + Sync {
    /// List all platforms, optionally filtering by enabled status.
    fn list(
        &self,
        enabled_only: bool,
    ) -> impl Future<Output = Result<Vec<GatewayPlatform>, AppError>> + Send;

    /// Get a platform by ID.
    fn get(
        &self,
        id: PlatformId,
    ) -> impl Future<Output = Result<GatewayPlatform, AppError>> + Send;

    /// Create a new platform.
    fn create(
        &self,
        id: PlatformId,
        input: &CreatePlatform,
    ) -> impl Future<Output = Result<GatewayPlatform, AppError>> + Send;

    /// Update a platform.
    fn update(
        &self,
        id: PlatformId,
        input: &UpdatePlatform,
    ) -> impl Future<Output = Result<GatewayPlatform, AppError>> + Send;

    /// Delete a platform and all its mappings (cascade).
    fn delete(&self, id: PlatformId) -> impl Future<Output = Result<(), AppError>> + Send;
}
```

- [ ] **Step 2: Add `GatewayMappingRepo` trait**

```rust
/// Repository for channel-to-conversation mappings.
pub trait GatewayMappingRepo: Send + Sync {
    /// List mappings for a platform.
    fn list_by_platform(
        &self,
        platform_id: PlatformId,
    ) -> impl Future<Output = Result<Vec<GatewayChannelMapping>, AppError>> + Send;

    /// Look up a mapping by platform + external channel ID.
    fn find_by_external_channel(
        &self,
        platform_id: PlatformId,
        external_channel_id: &str,
    ) -> impl Future<Output = Result<Option<GatewayChannelMapping>, AppError>> + Send;

    /// Look up mappings by conversation ID (reverse lookup for outbound).
    fn find_by_conversation(
        &self,
        conversation_id: ConversationId,
    ) -> impl Future<Output = Result<Vec<GatewayChannelMapping>, AppError>> + Send;

    /// Create a mapping. The `_tx` variant is used for transactional creation.
    fn create(
        &self,
        id: MappingId,
        platform_id: PlatformId,
        input: &CreateChannelMapping,
    ) -> impl Future<Output = Result<GatewayChannelMapping, AppError>> + Send;

    /// Delete a mapping by ID.
    fn delete(&self, id: MappingId) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Delete mappings for a specific external channel (used on ChannelDeleted).
    fn delete_by_external_channel(
        &self,
        platform_id: PlatformId,
        external_channel_id: &str,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// List all mappings (for gateway startup cache loading).
    fn list_all(
        &self,
    ) -> impl Future<Output = Result<Vec<GatewayChannelMapping>, AppError>> + Send;
}
```

- [ ] **Step 3: Add `GatewayUserMappingRepo` trait**

```rust
/// Repository for external-user-to-Sõber-user mappings.
pub trait GatewayUserMappingRepo: Send + Sync {
    /// List user mappings for a platform.
    fn list_by_platform(
        &self,
        platform_id: PlatformId,
    ) -> impl Future<Output = Result<Vec<GatewayUserMapping>, AppError>> + Send;

    /// Look up a user mapping by platform + external user ID.
    fn find_by_external_user(
        &self,
        platform_id: PlatformId,
        external_user_id: &str,
    ) -> impl Future<Output = Result<Option<GatewayUserMapping>, AppError>> + Send;

    /// Create a user mapping.
    fn create(
        &self,
        id: UserMappingId,
        platform_id: PlatformId,
        input: &CreateUserMapping,
    ) -> impl Future<Output = Result<GatewayUserMapping, AppError>> + Send;

    /// Delete a user mapping by ID.
    fn delete(&self, id: UserMappingId) -> impl Future<Output = Result<(), AppError>> + Send;

    /// List all user mappings (for gateway startup cache loading).
    fn list_all(
        &self,
    ) -> impl Future<Output = Result<Vec<GatewayUserMapping>, AppError>> + Send;
}
```

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-core -q`
Expected: compiles (trait definitions only, no implementations yet)

- [ ] **Step 5: Commit**

```
feat(core): add gateway repository traits
```

---

## Task 4: Gateway Config

**Files:**
- Modify: `backend/crates/sober-core/src/config.rs`

- [ ] **Step 1: Add config constants**

Add after the existing constants:

```rust
/// Default gateway gRPC socket path.
pub const DEFAULT_GATEWAY_SOCKET_PATH: &str = "/run/sober/gateway.sock";
/// Default gateway metrics port.
pub const DEFAULT_GATEWAY_METRICS_PORT: u16 = 9102;
/// Default gateway agent socket path.
pub const DEFAULT_GATEWAY_AGENT_SOCKET_PATH: &str = "/run/sober/agent.sock";
```

- [ ] **Step 2: Add `GatewayConfig` struct**

Add after `EvolutionConfig`:

```rust
/// Gateway process settings (sober-gateway binary).
///
/// Configurable via `[gateway]` TOML section or `SOBER_GATEWAY_*` env vars.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    /// Path to the gateway gRPC socket.
    pub socket_path: PathBuf,
    /// Prometheus metrics port.
    pub metrics_port: u16,
    /// Path to the agent gRPC socket (for HandleMessage + Subscribe).
    pub agent_socket_path: PathBuf,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            socket_path: PathBuf::from(DEFAULT_GATEWAY_SOCKET_PATH),
            metrics_port: DEFAULT_GATEWAY_METRICS_PORT,
            agent_socket_path: PathBuf::from(DEFAULT_GATEWAY_AGENT_SOCKET_PATH),
        }
    }
}
```

- [ ] **Step 3: Add to `AppConfig`**

Add `pub gateway: GatewayConfig,` field to `AppConfig` and its `Default` impl.

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-core -q`
Expected: compiles

- [ ] **Step 5: Commit**

```
feat(core): add gateway configuration section
```

---

## Task 5: Database Migrations

**Files:**
- Create: `backend/migrations/20260402000003_gateway_bot_user.sql`
- Create: `backend/migrations/20260402000004_gateway_tables.sql`

- [ ] **Step 1: Create bot user migration**

The bot user needs a deterministic UUID so the gateway binary can reference it without a DB query at startup. Uses a well-known UUID. The password hash is a dummy bcrypt value — the bot user cannot log in via the API.

```sql
-- Seed the gateway bridge bot user.
-- This user is the sender for messages from unmapped external users.
-- It cannot log in (dummy password hash) and is created with 'active' status.
INSERT INTO users (id, email, username, password_hash, status, created_at, updated_at)
VALUES (
    '01960000-0000-7000-8000-000000000100',
    'gateway-bot@sober.internal',
    'gateway-bot',
    '$2b$12$000000000000000000000000000000000000000000000000000000',
    'active',
    now(),
    now()
)
ON CONFLICT (id) DO NOTHING;

-- Give the bot the default 'user' role (not admin).
INSERT INTO user_roles (user_id, role_id, scope_id, granted_at)
VALUES (
    '01960000-0000-7000-8000-000000000100',
    '01960000-0000-7000-8000-000000000001',
    '00000000-0000-0000-0000-000000000000',
    now()
)
ON CONFLICT DO NOTHING;
```

- [ ] **Step 2: Create gateway tables migration**

```sql
-- Platform connections (one row per bot/token).
CREATE TABLE gateway_platforms (
    id             UUID PRIMARY KEY,
    platform_type  TEXT NOT NULL,
    display_name   TEXT NOT NULL,
    is_enabled     BOOLEAN NOT NULL DEFAULT true,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Channel-to-conversation mappings.
CREATE TABLE gateway_channel_mappings (
    id                    UUID PRIMARY KEY,
    platform_id           UUID NOT NULL REFERENCES gateway_platforms(id) ON DELETE CASCADE,
    external_channel_id   TEXT NOT NULL,
    external_channel_name TEXT NOT NULL,
    conversation_id       UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    is_thread             BOOLEAN NOT NULL DEFAULT false,
    parent_mapping_id     UUID REFERENCES gateway_channel_mappings(id) ON DELETE SET NULL,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_gateway_mappings_platform_channel
    ON gateway_channel_mappings(platform_id, external_channel_id);

CREATE INDEX idx_gateway_mappings_conversation
    ON gateway_channel_mappings(conversation_id);

-- External-user-to-Sõber-user mappings.
CREATE TABLE gateway_user_mappings (
    id                UUID PRIMARY KEY,
    platform_id       UUID NOT NULL REFERENCES gateway_platforms(id) ON DELETE CASCADE,
    external_user_id  TEXT NOT NULL,
    external_username TEXT NOT NULL,
    user_id           UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_gateway_user_mappings_platform_user
    ON gateway_user_mappings(platform_id, external_user_id);
```

- [ ] **Step 3: Run migrations to verify**

Run: `cd backend && cargo run -q --bin sober -- migrate run`
Expected: both migrations apply successfully

- [ ] **Step 4: Commit**

```
feat(db): add gateway migrations — bot user and mapping tables
```

---

## Task 6: Database Row Types and Repo Implementations

**Files:**
- Modify: `backend/crates/sober-db/src/rows.rs`
- Create: `backend/crates/sober-db/src/repos/gateway.rs`
- Modify: `backend/crates/sober-db/src/repos/mod.rs`
- Modify: `backend/crates/sober-db/src/lib.rs`

- [ ] **Step 1: Add row types to `rows.rs`**

```rust
#[derive(sqlx::FromRow)]
pub(crate) struct GatewayPlatformRow {
    pub id: Uuid,
    pub platform_type: String,
    pub display_name: String,
    pub is_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<GatewayPlatformRow> for GatewayPlatform {
    fn from(row: GatewayPlatformRow) -> Self {
        Self {
            id: PlatformId::from_uuid(row.id),
            platform_type: row.platform_type.parse().unwrap_or(PlatformType::Discord),
            display_name: row.display_name,
            is_enabled: row.is_enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct GatewayChannelMappingRow {
    pub id: Uuid,
    pub platform_id: Uuid,
    pub external_channel_id: String,
    pub external_channel_name: String,
    pub conversation_id: Uuid,
    pub is_thread: bool,
    pub parent_mapping_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl From<GatewayChannelMappingRow> for GatewayChannelMapping {
    fn from(row: GatewayChannelMappingRow) -> Self {
        Self {
            id: MappingId::from_uuid(row.id),
            platform_id: PlatformId::from_uuid(row.platform_id),
            external_channel_id: row.external_channel_id,
            external_channel_name: row.external_channel_name,
            conversation_id: ConversationId::from_uuid(row.conversation_id),
            is_thread: row.is_thread,
            parent_mapping_id: row.parent_mapping_id.map(MappingId::from_uuid),
            created_at: row.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct GatewayUserMappingRow {
    pub id: Uuid,
    pub platform_id: Uuid,
    pub external_user_id: String,
    pub external_username: String,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
}

impl From<GatewayUserMappingRow> for GatewayUserMapping {
    fn from(row: GatewayUserMappingRow) -> Self {
        Self {
            id: UserMappingId::from_uuid(row.id),
            platform_id: PlatformId::from_uuid(row.platform_id),
            external_user_id: row.external_user_id,
            external_username: row.external_username,
            user_id: UserId::from_uuid(row.user_id),
            created_at: row.created_at,
        }
    }
}
```

- [ ] **Step 2: Implement repos in `gateway.rs`**

Create `backend/crates/sober-db/src/repos/gateway.rs`. Implement all three repo traits (`GatewayPlatformRepo`, `GatewayMappingRepo`, `GatewayUserMappingRepo`) following the existing pattern:

- Struct holds `PgPool`
- `new(pool: PgPool) -> Self`
- `sqlx::query_as::<_, RowType>(SQL).bind(...).fetch_*(&self.pool)`
- Convert rows via `.into()` / `.map(Into::into)`
- Return `AppError::NotFound` for missing records

`PlatformType` is stored as `TEXT` in the DB. The `PlatformType` enum needs a `FromStr` impl for parsing the text value back:

```rust
impl std::str::FromStr for PlatformType {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "discord" => Ok(Self::Discord),
            "telegram" => Ok(Self::Telegram),
            "matrix" => Ok(Self::Matrix),
            "whatsapp" => Ok(Self::Whatsapp),
            _ => Err(AppError::Validation(format!("unknown platform type: {s}"))),
        }
    }
}
```

Add this `FromStr` impl alongside the `PlatformType` definition in sober-core.

For the `PgGatewayPlatformRepo::create` method:

```rust
async fn create(
    &self,
    id: PlatformId,
    input: &CreatePlatform,
) -> Result<GatewayPlatform, AppError> {
    let row = sqlx::query_as::<_, GatewayPlatformRow>(
        "INSERT INTO gateway_platforms (id, platform_type, display_name) \
         VALUES ($1, $2, $3) \
         RETURNING id, platform_type, display_name, is_enabled, created_at, updated_at"
    )
    .bind(id.as_uuid())
    .bind(input.platform_type.to_string())
    .bind(&input.display_name)
    .fetch_one(&self.pool)
    .await?;
    Ok(row.into())
}
```

Follow the same pattern for all methods. Key queries:

- `list` with optional `WHERE is_enabled = true`
- `find_by_external_channel`: `WHERE platform_id = $1 AND external_channel_id = $2`
- `find_by_conversation`: `WHERE conversation_id = $1`
- `delete_by_external_channel`: `DELETE FROM gateway_channel_mappings WHERE platform_id = $1 AND external_channel_id = $2`

Also add `_tx` variants for `create` methods on `GatewayMappingRepo` and `GatewayUserMappingRepo` that accept `&mut PgConnection` instead of using the pool — needed for transactional creation in `GatewayAdminService`.

```rust
impl PgGatewayMappingRepo {
    /// Transactional variant — creates mapping within an existing transaction.
    pub async fn create_tx(
        conn: &mut PgConnection,
        id: MappingId,
        platform_id: PlatformId,
        input: &CreateChannelMapping,
    ) -> Result<GatewayChannelMapping, AppError> {
        let row = sqlx::query_as::<_, GatewayChannelMappingRow>(
            "INSERT INTO gateway_channel_mappings \
             (id, platform_id, external_channel_id, external_channel_name, conversation_id) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING *"
        )
        .bind(id.as_uuid())
        .bind(platform_id.as_uuid())
        .bind(&input.external_channel_id)
        .bind(&input.external_channel_name)
        .bind(input.conversation_id.as_uuid())
        .fetch_one(&mut *conn)
        .await?;
        Ok(row.into())
    }
}
```

- [ ] **Step 3: Register module and re-export**

In `backend/crates/sober-db/src/repos/mod.rs`, add `pub mod gateway;`.

In `backend/crates/sober-db/src/lib.rs`, add re-exports:
```rust
pub use repos::gateway::{PgGatewayMappingRepo, PgGatewayPlatformRepo, PgGatewayUserMappingRepo};
```

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-db -q`
Expected: compiles

- [ ] **Step 5: Commit**

```
feat(db): implement gateway repository layer
```

---

## Task 7: Proto Definition

**Files:**
- Create: `backend/proto/sober/gateway/v1/gateway.proto`

- [ ] **Step 1: Write the proto file**

```protobuf
syntax = "proto3";
package sober.gateway.v1;

service GatewayService {
  // List available channels from a connected platform.
  rpc ListChannels(ListChannelsRequest) returns (ListChannelsResponse);
  // Re-read platform configs from DB, connect/disconnect as needed.
  rpc Reload(ReloadRequest) returns (ReloadResponse);
  // List connected platforms and active mapping counts.
  rpc Status(StatusRequest) returns (StatusResponse);
  // Liveness probe.
  rpc Health(HealthRequest) returns (HealthResponse);
}

message ListChannelsRequest {
  string platform_id = 1;
}

message ListChannelsResponse {
  repeated ExternalChannel channels = 1;
}

message ExternalChannel {
  string id = 1;
  string name = 2;
  string kind = 3; // "text", "voice", "category", etc.
}

message ReloadRequest {}
message ReloadResponse {}

message StatusRequest {}

message StatusResponse {
  repeated PlatformStatus platforms = 1;
}

message PlatformStatus {
  string platform_id = 1;
  string platform_type = 2;
  string display_name = 3;
  string status = 4; // "connected", "reconnecting", "disconnected"
  uint32 mapping_count = 5;
}

message HealthRequest {}
message HealthResponse {
  bool healthy = 1;
}
```

- [ ] **Step 2: Commit**

```
feat(proto): add gateway gRPC service definition
```

---

## Task 8: Gateway Crate Scaffold

**Files:**
- Create: `backend/crates/sober-gateway/Cargo.toml`
- Create: `backend/crates/sober-gateway/build.rs`
- Create: `backend/crates/sober-gateway/src/lib.rs`
- Create: `backend/crates/sober-gateway/src/main.rs`
- Create: `backend/crates/sober-gateway/src/error.rs`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "sober-gateway"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[[bin]]
name = "sober-gateway"
path = "src/main.rs"

[lib]
name = "sober_gateway"
path = "src/lib.rs"

[dependencies]
sober-core = { path = "../sober-core", features = ["postgres"] }
sober-db = { path = "../sober-db" }
sober-crypto = { path = "../sober-crypto" }

anyhow = { workspace = true }
dashmap = { workspace = true }
metrics = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
sqlx = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true }
tokio-stream = { workspace = true }
tokio-util = { workspace = true }
tonic = { workspace = true }
tracing = { workspace = true }
hyper-util = { workspace = true }
tower = { workspace = true }
uuid = { workspace = true }

# Discord SDK
serenity = { version = "0.12", default-features = false, features = ["client", "gateway", "model", "rustls_backend"] }

[build-dependencies]
tonic-prost-build = { workspace = true }
```

Check `backend/Cargo.toml` workspace dependencies to confirm `dashmap`, `anyhow`, `thiserror`, `tokio-stream`, `tokio-util`, `hyper-util`, `tower`, `uuid` are listed. Add any that are missing. Also check if `serenity` needs to be added to workspace dependencies or can be a direct dependency.

- [ ] **Step 2: Create `build.rs`**

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::compile_protos("../../proto/sober/gateway/v1/gateway.proto")?;
    tonic_prost_build::compile_protos("../../proto/sober/agent/v1/agent.proto")?;
    Ok(())
}
```

- [ ] **Step 3: Create `src/error.rs`**

```rust
use sober_core::error::AppError;
use thiserror::Error;

/// Gateway-specific errors.
#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("platform connection failed: {0}")]
    ConnectionFailed(String),

    #[error("platform send failed: {0}")]
    SendFailed(String),

    #[error("platform not found: {0}")]
    PlatformNotFound(String),

    #[error("channel not found: {0}")]
    ChannelNotFound(String),

    #[error("unmapped channel: platform={platform_id}, channel={channel_id}")]
    UnmappedChannel {
        platform_id: String,
        channel_id: String,
    },
}

impl From<GatewayError> for AppError {
    fn from(err: GatewayError) -> Self {
        match err {
            GatewayError::PlatformNotFound(_) | GatewayError::ChannelNotFound(_) => {
                AppError::NotFound(err.to_string())
            }
            _ => AppError::Internal(err.into()),
        }
    }
}
```

- [ ] **Step 4: Create `src/lib.rs`**

```rust
//! Sober Gateway — bridges external messaging platforms to Sõber conversations.

pub mod error;

pub mod proto {
    tonic::include_proto!("sober.gateway.v1");
}

pub mod agent_proto {
    tonic::include_proto!("sober.agent.v1");
}
```

- [ ] **Step 5: Create minimal `src/main.rs`**

Start with a minimal main that loads config, connects to DB, and starts the gRPC server with a placeholder service. The full event loop is built in subsequent tasks.

```rust
//! Sober Gateway — external messaging platform bridge.
//!
//! Connects to external platforms (Discord, Telegram, etc.), routes inbound
//! messages to sober-agent via gRPC/UDS, and delivers agent responses back
//! to the originating platform.

use std::path::Path;

use anyhow::{Context, Result};
use hyper_util::rt::TokioIo;
use sober_core::config::AppConfig;
use sober_db::create_pool;
use tokio::net::UnixListener;
use tokio::signal;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::{Endpoint, Server, Uri};
use tower::service_fn;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let config = AppConfig::load().context("failed to load config")?;

    let telemetry = sober_core::init_telemetry(
        config.environment,
        "sober_gateway=info,sqlx::query=warn,info",
    );

    sober_core::spawn_metrics_server(telemetry.prometheus.clone(), config.gateway.metrics_port);

    info!("sober-gateway starting");

    // Connect to PostgreSQL
    let db_config = sober_db::DatabaseConfig {
        url: config.database.url.clone(),
        max_connections: config.database.max_connections,
    };
    let pool = create_pool(&db_config)
        .await
        .context("failed to connect to PostgreSQL")?;

    // Connect to agent gRPC
    let agent_socket_path = config.gateway.agent_socket_path.clone();
    let agent_channel = Endpoint::try_from("http://[::]:50051")?
        .connect_with_connector(service_fn(move |_: Uri| {
            let path = agent_socket_path.clone();
            async move {
                let stream = tokio::net::UnixStream::connect(path).await?;
                Ok::<_, std::io::Error>(TokioIo::new(stream))
            }
        }))
        .await
        .context("failed to connect to agent gRPC")?;
    let agent_client =
        sober_gateway::agent_proto::agent_service_client::AgentServiceClient::new(agent_channel);

    info!("connected to agent gRPC service");

    // Clean up stale socket
    let socket_path = &config.gateway.socket_path;
    if Path::new(socket_path).exists() {
        std::fs::remove_file(socket_path)?;
    }

    // Start gRPC server (placeholder — real service added in Task 12)
    let listener = UnixListener::bind(socket_path)?;
    let stream = UnixListenerStream::new(listener);

    info!(socket = %socket_path.display(), "gRPC server listening");

    Server::builder()
        .serve_with_incoming_graceful_shutdown(stream, shutdown_signal())
        .await?;

    info!("sober-gateway stopped");
    Ok(())
}

async fn shutdown_signal() {
    let sigterm = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    tokio::select! {
        () = sigterm => {}
        _ = signal::ctrl_c() => {}
    }
}
```

- [ ] **Step 6: Build and verify**

Run: `cd backend && cargo build -p sober-gateway -q`
Expected: compiles (the gRPC server has no services yet — that's fine, tonic allows empty servers)

- [ ] **Step 7: Commit**

```
feat(gateway): scaffold crate with main, error, and proto compilation
```

---

## Task 9: Platform Bridge Trait and Types

**Files:**
- Create: `backend/crates/sober-gateway/src/types.rs`
- Create: `backend/crates/sober-gateway/src/bridge.rs`
- Modify: `backend/crates/sober-gateway/src/lib.rs`

- [ ] **Step 1: Create `types.rs`**

```rust
use sober_core::types::PlatformId;

/// Events emitted by platform bridges into the gateway event loop.
#[derive(Debug)]
pub enum GatewayEvent {
    /// A message was received from an external platform.
    MessageReceived {
        platform_id: PlatformId,
        channel_id: String,
        user_id: String,
        username: String,
        content: String,
    },
    /// An external channel was deleted.
    ChannelDeleted {
        platform_id: PlatformId,
        channel_id: String,
    },
}

/// Message to send to an external platform.
#[derive(Debug, Clone)]
pub struct PlatformMessage {
    pub text: String,
    pub format: MessageFormat,
    pub reply_to: Option<String>,
}

/// Format of message content.
#[derive(Debug, Clone, Copy)]
pub enum MessageFormat {
    Markdown,
    Plain,
}

/// A channel visible to the bot on an external platform.
#[derive(Debug, Clone)]
pub struct ExternalChannel {
    pub id: String,
    pub name: String,
    pub kind: String,
}

/// Configuration for connecting to an external platform.
#[derive(Debug, Clone)]
pub struct PlatformConfig {
    pub platform_id: PlatformId,
    pub credentials: PlatformCredentials,
}

/// Platform-specific credentials.
#[derive(Debug, Clone)]
pub enum PlatformCredentials {
    Discord { bot_token: String },
    Telegram { bot_token: String },
    Matrix { homeserver_url: String, access_token: String },
    Whatsapp { phone_number_id: String, access_token: String },
}
```

- [ ] **Step 2: Create `bridge.rs`**

```rust
use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use sober_core::types::{PlatformId, PlatformType};
use tokio::sync::mpsc;

use crate::error::GatewayError;
use crate::types::{ExternalChannel, GatewayEvent, PlatformConfig, PlatformMessage};

/// Trait implemented by each platform SDK adapter.
pub trait PlatformBridge: Send + Sync + 'static {
    /// Connect to the platform and start receiving events.
    fn connect(
        &mut self,
        config: PlatformConfig,
        event_tx: mpsc::Sender<GatewayEvent>,
    ) -> impl Future<Output = Result<(), GatewayError>> + Send;

    /// Disconnect gracefully.
    fn disconnect(&mut self) -> impl Future<Output = Result<(), GatewayError>> + Send;

    /// Send a message to an external channel.
    fn send_message(
        &self,
        channel_id: &str,
        content: PlatformMessage,
    ) -> impl Future<Output = Result<(), GatewayError>> + Send;

    /// Platform identifier.
    fn platform_type(&self) -> PlatformType;

    /// Resolve display name for a channel ID.
    fn resolve_channel_name(
        &self,
        channel_id: &str,
    ) -> impl Future<Output = Result<String, GatewayError>> + Send;

    /// List channels the bot has access to.
    fn list_channels(
        &self,
    ) -> impl Future<Output = Result<Vec<ExternalChannel>, GatewayError>> + Send;
}

use std::future::Future;

/// Registry of active platform bridge connections.
pub struct PlatformBridgeRegistry {
    bridges: DashMap<PlatformId, Arc<dyn PlatformBridgeHandle>>,
}

/// Type-erased handle to a connected bridge (for send_message + list_channels).
#[async_trait::async_trait]
pub trait PlatformBridgeHandle: Send + Sync {
    async fn send_message(
        &self,
        channel_id: &str,
        content: PlatformMessage,
    ) -> Result<(), GatewayError>;

    async fn list_channels(&self) -> Result<Vec<ExternalChannel>, GatewayError>;

    fn platform_type(&self) -> PlatformType;
}

impl PlatformBridgeRegistry {
    pub fn new() -> Self {
        Self {
            bridges: DashMap::new(),
        }
    }

    /// Register a connected bridge.
    pub fn insert(&self, platform_id: PlatformId, bridge: Arc<dyn PlatformBridgeHandle>) {
        self.bridges.insert(platform_id, bridge);
    }

    /// Remove a bridge on disconnect.
    pub fn remove(&self, platform_id: &PlatformId) {
        self.bridges.remove(platform_id);
    }

    /// Get a bridge by platform ID.
    pub fn get(&self, platform_id: &PlatformId) -> Option<Arc<dyn PlatformBridgeHandle>> {
        self.bridges.get(platform_id).map(|v| v.value().clone())
    }

    /// List all connected platform statuses.
    pub fn statuses(&self) -> Vec<(PlatformId, PlatformType)> {
        self.bridges
            .iter()
            .map(|entry| (*entry.key(), entry.value().platform_type()))
            .collect()
    }
}
```

Note: Add `async-trait` to `Cargo.toml` dependencies if not already a workspace dependency. Alternatively, use a boxed future approach — check what the codebase prefers. The codebase uses RPITIT for repo traits (no `async_trait`), but `PlatformBridgeHandle` needs to be object-safe (used in `DashMap<PlatformId, Arc<dyn PlatformBridgeHandle>>`), so `async_trait` is needed here.

- [ ] **Step 3: Update `lib.rs`**

```rust
pub mod bridge;
pub mod error;
pub mod types;
```

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-gateway -q`
Expected: compiles

- [ ] **Step 5: Commit**

```
feat(gateway): add PlatformBridge trait and registry
```

---

## Task 10: Gateway Service — Inbound and Outbound Logic

**Files:**
- Create: `backend/crates/sober-gateway/src/service.rs`
- Create: `backend/crates/sober-gateway/src/outbound.rs`
- Modify: `backend/crates/sober-gateway/src/lib.rs`

- [ ] **Step 1: Create `outbound.rs` — response buffer**

```rust
use std::collections::HashMap;
use std::time::{Duration, Instant};

use sober_core::types::ConversationId;

use crate::types::{MessageFormat, PlatformMessage};

/// Buffers streaming text deltas per conversation and flushes on Done.
pub struct OutboundBuffer {
    buffers: HashMap<ConversationId, ConversationBuffer>,
}

struct ConversationBuffer {
    text: String,
    last_flush: Instant,
}

impl OutboundBuffer {
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
        }
    }

    /// Append a text delta. Returns None (keep buffering).
    pub fn append_delta(&mut self, conversation_id: ConversationId, delta: &str) {
        let buf = self
            .buffers
            .entry(conversation_id)
            .or_insert_with(|| ConversationBuffer {
                text: String::new(),
                last_flush: Instant::now(),
            });
        buf.text.push_str(delta);
    }

    /// Flush and consume the buffer for a conversation. Returns the complete message.
    pub fn flush(&mut self, conversation_id: &ConversationId) -> Option<PlatformMessage> {
        self.buffers.remove(conversation_id).and_then(|buf| {
            if buf.text.is_empty() {
                None
            } else {
                Some(PlatformMessage {
                    text: buf.text,
                    format: MessageFormat::Markdown,
                    reply_to: None,
                })
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_accumulates_and_flushes() {
        let mut buf = OutboundBuffer::new();
        let conv = ConversationId::new();

        buf.append_delta(conv, "Hello ");
        buf.append_delta(conv, "world!");

        let msg = buf.flush(&conv).unwrap();
        assert_eq!(msg.text, "Hello world!");
    }

    #[test]
    fn flush_empty_returns_none() {
        let mut buf = OutboundBuffer::new();
        let conv = ConversationId::new();
        assert!(buf.flush(&conv).is_none());
    }

    #[test]
    fn flush_clears_buffer() {
        let mut buf = OutboundBuffer::new();
        let conv = ConversationId::new();
        buf.append_delta(conv, "test");
        buf.flush(&conv);
        assert!(buf.flush(&conv).is_none());
    }
}
```

- [ ] **Step 2: Run outbound tests**

Run: `cd backend && cargo test -p sober-gateway -q`
Expected: 3 tests pass

- [ ] **Step 3: Create `service.rs`**

```rust
use std::sync::Arc;

use dashmap::DashMap;
use sober_core::error::AppError;
use sober_core::types::{
    ConversationId, GatewayChannelMapping, GatewayUserMapping, MappingId, PlatformId, UserId,
};
use sober_db::{PgGatewayMappingRepo, PgGatewayPlatformRepo, PgGatewayUserMappingRepo};
use sqlx::PgPool;
use tracing::{debug, info, warn};

use crate::agent_proto::agent_service_client::AgentServiceClient;
use crate::bridge::PlatformBridgeRegistry;
use crate::outbound::OutboundBuffer;
use crate::types::GatewayEvent;

/// Well-known UUID of the gateway bridge bot user (seeded in migration).
pub const BRIDGE_BOT_USER_ID: &str = "01960000-0000-7000-8000-000000000100";

/// Core gateway business logic.
pub struct GatewayService {
    db: PgPool,
    agent_client: AgentServiceClient<tonic::transport::Channel>,
    bridges: Arc<PlatformBridgeRegistry>,
    /// Inbound cache: (platform_id, external_channel_id) → mapping
    channel_cache: DashMap<(PlatformId, String), GatewayChannelMapping>,
    /// Reverse cache: conversation_id → list of (platform_id, external_channel_id)
    reverse_cache: DashMap<ConversationId, Vec<(PlatformId, String)>>,
    /// Inbound cache: (platform_id, external_user_id) → UserId
    user_cache: DashMap<(PlatformId, String), UserId>,
    /// Bridge bot user ID for unmapped users.
    bot_user_id: UserId,
}

impl GatewayService {
    pub fn new(
        db: PgPool,
        agent_client: AgentServiceClient<tonic::transport::Channel>,
        bridges: Arc<PlatformBridgeRegistry>,
    ) -> Self {
        let bot_uuid = uuid::Uuid::parse_str(BRIDGE_BOT_USER_ID).expect("valid bot UUID");
        Self {
            db,
            agent_client,
            bridges,
            channel_cache: DashMap::new(),
            reverse_cache: DashMap::new(),
            user_cache: DashMap::new(),
            bot_user_id: UserId::from_uuid(bot_uuid),
        }
    }

    /// Load all mappings and user mappings into in-memory caches.
    pub async fn load_caches(&self) -> Result<(), AppError> {
        let mapping_repo = PgGatewayMappingRepo::new(self.db.clone());
        let user_repo = PgGatewayUserMappingRepo::new(self.db.clone());

        let mappings = mapping_repo.list_all().await?;
        for m in &mappings {
            self.channel_cache.insert(
                (m.platform_id, m.external_channel_id.clone()),
                m.clone(),
            );
        }

        // Build reverse cache
        for m in &mappings {
            self.reverse_cache
                .entry(m.conversation_id)
                .or_default()
                .push((m.platform_id, m.external_channel_id.clone()));
        }

        let user_mappings = user_repo.list_all().await?;
        for u in &user_mappings {
            self.user_cache
                .insert((u.platform_id, u.external_user_id.clone()), u.user_id);
        }

        info!(
            channel_mappings = mappings.len(),
            user_mappings = user_mappings.len(),
            "loaded gateway caches"
        );
        Ok(())
    }

    /// Handle an inbound gateway event.
    pub async fn handle_event(&self, event: GatewayEvent) {
        match event {
            GatewayEvent::MessageReceived {
                platform_id,
                channel_id,
                user_id,
                username,
                content,
            } => {
                self.handle_message(platform_id, &channel_id, &user_id, &username, &content)
                    .await;
            }
            GatewayEvent::ChannelDeleted {
                platform_id,
                channel_id,
            } => {
                self.handle_channel_deleted(platform_id, &channel_id).await;
            }
        }
    }

    async fn handle_message(
        &self,
        platform_id: PlatformId,
        channel_id: &str,
        external_user_id: &str,
        username: &str,
        content: &str,
    ) {
        // 1. Look up channel mapping
        let mapping = self
            .channel_cache
            .get(&(platform_id, channel_id.to_owned()));

        let Some(mapping) = mapping else {
            metrics::counter!("sober_gateway_unmapped_messages_total",
                "platform" => platform_id.to_string()
            )
            .increment(1);
            warn!(
                platform_id = %platform_id,
                channel_id = channel_id,
                "dropping message for unmapped channel"
            );
            return;
        };
        let conversation_id = mapping.conversation_id;
        drop(mapping); // release DashMap guard

        // 2. Look up user mapping
        let (sender_user_id, message_content) = match self
            .user_cache
            .get(&(platform_id, external_user_id.to_owned()))
        {
            Some(uid) => (*uid, content.to_owned()),
            None => {
                // Unmapped user: use bot, prefix with username
                (self.bot_user_id, format!("[{username}] {content}"))
            }
        };

        // 3. Send to agent
        let mut client = self.agent_client.clone();
        let request = crate::agent_proto::HandleMessageRequest {
            user_id: sender_user_id.to_string(),
            conversation_id: conversation_id.to_string(),
            content: message_content,
            attachments: vec![],
        };

        let timer = std::time::Instant::now();
        match client.handle_message(request).await {
            Ok(_) => {
                metrics::counter!("sober_gateway_messages_received_total",
                    "platform" => platform_id.to_string(),
                    "status" => "success"
                )
                .increment(1);
                metrics::histogram!("sober_gateway_message_handle_duration_seconds",
                    "platform" => platform_id.to_string()
                )
                .record(timer.elapsed().as_secs_f64());
                debug!(
                    conversation_id = %conversation_id,
                    "forwarded message to agent"
                );
            }
            Err(e) => {
                metrics::counter!("sober_gateway_messages_received_total",
                    "platform" => platform_id.to_string(),
                    "status" => "error"
                )
                .increment(1);
                tracing::error!(error = %e, "failed to send message to agent");
            }
        }
    }

    async fn handle_channel_deleted(&self, platform_id: PlatformId, channel_id: &str) {
        // Remove from in-memory cache
        let removed = self
            .channel_cache
            .remove(&(platform_id, channel_id.to_owned()));

        if let Some((_, mapping)) = removed {
            // Remove from reverse cache
            if let Some(mut entries) = self.reverse_cache.get_mut(&mapping.conversation_id) {
                entries.retain(|(pid, cid)| !(*pid == platform_id && cid == channel_id));
            }
        }

        // Remove from DB
        let repo = PgGatewayMappingRepo::new(self.db.clone());
        if let Err(e) = repo
            .delete_by_external_channel(platform_id, channel_id)
            .await
        {
            tracing::error!(error = %e, "failed to delete channel mapping from DB");
        }

        info!(
            platform_id = %platform_id,
            channel_id = channel_id,
            "removed mapping for deleted channel"
        );
    }

    /// Get the bridge registry (for outbound delivery and gRPC service).
    pub fn bridges(&self) -> &Arc<PlatformBridgeRegistry> {
        &self.bridges
    }

    /// Look up reverse mappings for outbound delivery.
    pub fn get_outbound_targets(
        &self,
        conversation_id: &ConversationId,
    ) -> Option<Vec<(PlatformId, String)>> {
        self.reverse_cache
            .get(conversation_id)
            .map(|v| v.value().clone())
    }
}
```

- [ ] **Step 4: Update `lib.rs`**

```rust
pub mod bridge;
pub mod error;
pub mod outbound;
pub mod service;
pub mod types;
```

- [ ] **Step 5: Build and verify**

Run: `cd backend && cargo build -p sober-gateway -q`
Expected: compiles

- [ ] **Step 6: Commit**

```
feat(gateway): implement GatewayService with inbound routing and outbound buffering
```

---

## Task 11: Discord Bridge Implementation

**Files:**
- Create: `backend/crates/sober-gateway/src/discord/mod.rs`
- Create: `backend/crates/sober-gateway/src/discord/handler.rs`
- Modify: `backend/crates/sober-gateway/src/lib.rs`

- [ ] **Step 1: Create `discord/handler.rs`**

Serenity event handler that captures messages and emits `GatewayEvent`s:

```rust
use serenity::all::{Context, EventHandler, Message, Ready};
use sober_core::types::PlatformId;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::types::GatewayEvent;

/// Serenity event handler that forwards Discord events to the gateway event loop.
pub struct DiscordHandler {
    platform_id: PlatformId,
    event_tx: mpsc::Sender<GatewayEvent>,
    bot_user_id: std::sync::OnceLock<serenity::all::UserId>,
}

impl DiscordHandler {
    pub fn new(platform_id: PlatformId, event_tx: mpsc::Sender<GatewayEvent>) -> Self {
        Self {
            platform_id,
            event_tx,
            bot_user_id: std::sync::OnceLock::new(),
        }
    }
}

#[serenity::async_trait]
impl EventHandler for DiscordHandler {
    async fn message(&self, _ctx: Context, msg: Message) {
        // Skip bot's own messages
        if let Some(bot_id) = self.bot_user_id.get() {
            if msg.author.id == *bot_id {
                return;
            }
        }

        // Skip other bot messages
        if msg.author.bot {
            return;
        }

        let event = GatewayEvent::MessageReceived {
            platform_id: self.platform_id,
            channel_id: msg.channel_id.to_string(),
            user_id: msg.author.id.to_string(),
            username: msg.author.name.clone(),
            content: msg.content.clone(),
        };

        if let Err(e) = self.event_tx.send(event).await {
            tracing::error!(error = %e, "failed to send gateway event");
        }
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        let _ = self.bot_user_id.set(ready.user.id);
        info!(
            bot_name = %ready.user.name,
            guild_count = ready.guilds.len(),
            "Discord bot connected"
        );
    }
}
```

- [ ] **Step 2: Create `discord/mod.rs`**

```rust
mod handler;

use std::sync::Arc;

use serenity::all::{ChannelType, GatewayIntents, GuildChannel, Http};
use serenity::Client;
use sober_core::types::{PlatformId, PlatformType};
use tokio::sync::mpsc;
use tracing::info;

use crate::bridge::PlatformBridgeHandle;
use crate::error::GatewayError;
use crate::types::{ExternalChannel, GatewayEvent, PlatformConfig, PlatformCredentials, PlatformMessage};

use handler::DiscordHandler;

/// Discord platform bridge using the serenity library.
pub struct DiscordBridge {
    platform_id: PlatformId,
    http: Option<Arc<Http>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl DiscordBridge {
    pub fn new(platform_id: PlatformId) -> Self {
        Self {
            platform_id,
            http: None,
            shutdown_tx: None,
        }
    }

    /// Connect to Discord and start receiving events.
    pub async fn connect(
        &mut self,
        config: PlatformConfig,
        event_tx: mpsc::Sender<GatewayEvent>,
    ) -> Result<(), GatewayError> {
        let PlatformCredentials::Discord { bot_token } = &config.credentials else {
            return Err(GatewayError::ConnectionFailed(
                "expected Discord credentials".into(),
            ));
        };

        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT
            | GatewayIntents::GUILDS;

        let handler = DiscordHandler::new(self.platform_id, event_tx);

        let mut client = Client::builder(bot_token, intents)
            .event_handler(handler)
            .await
            .map_err(|e| GatewayError::ConnectionFailed(e.to_string()))?;

        self.http = Some(client.http.clone());

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        // Spawn serenity client in background
        tokio::spawn(async move {
            tokio::select! {
                result = client.start() => {
                    if let Err(e) = result {
                        tracing::error!(error = %e, "Discord client error");
                    }
                }
                _ = &mut shutdown_rx => {
                    client.shard_manager.shutdown_all().await;
                    info!("Discord client shut down");
                }
            }
        });

        Ok(())
    }

    /// Disconnect from Discord.
    pub async fn disconnect(&mut self) -> Result<(), GatewayError> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.http = None;
        Ok(())
    }

    fn http(&self) -> Result<&Arc<Http>, GatewayError> {
        self.http
            .as_ref()
            .ok_or_else(|| GatewayError::ConnectionFailed("not connected".into()))
    }
}

#[async_trait::async_trait]
impl PlatformBridgeHandle for DiscordBridge {
    async fn send_message(
        &self,
        channel_id: &str,
        content: PlatformMessage,
    ) -> Result<(), GatewayError> {
        let http = self.http()?;
        let channel_id: u64 = channel_id
            .parse()
            .map_err(|_| GatewayError::ChannelNotFound(channel_id.to_owned()))?;

        let channel = serenity::all::ChannelId::new(channel_id);
        channel
            .say(http, &content.text)
            .await
            .map_err(|e| GatewayError::SendFailed(e.to_string()))?;

        Ok(())
    }

    async fn list_channels(&self) -> Result<Vec<ExternalChannel>, GatewayError> {
        let http = self.http()?;

        // Get all guilds the bot is in
        let guilds = http
            .get_guilds(None, None)
            .await
            .map_err(|e| GatewayError::ConnectionFailed(e.to_string()))?;

        let mut channels = Vec::new();
        for guild in guilds {
            let guild_channels = http
                .get_channels(guild.id)
                .await
                .map_err(|e| GatewayError::ConnectionFailed(e.to_string()))?;

            for ch in guild_channels {
                let kind = match ch.kind {
                    ChannelType::Text => "text",
                    ChannelType::Voice => "voice",
                    ChannelType::Category => "category",
                    ChannelType::News => "news",
                    ChannelType::Forum => "forum",
                    _ => "other",
                };

                // Only include text-based channels
                if matches!(ch.kind, ChannelType::Text | ChannelType::News | ChannelType::Forum) {
                    channels.push(ExternalChannel {
                        id: ch.id.to_string(),
                        name: format!("{}#{}", guild.name, ch.name),
                        kind: kind.to_owned(),
                    });
                }
            }
        }

        Ok(channels)
    }

    fn platform_type(&self) -> PlatformType {
        PlatformType::Discord
    }
}
```

- [ ] **Step 3: Update `lib.rs`**

Add `pub mod discord;` to the module list.

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-gateway -q`
Expected: compiles (serenity may need version adjustment — check the latest stable release)

- [ ] **Step 5: Commit**

```
feat(gateway): implement Discord bridge with serenity
```

---

## Task 12: Gateway gRPC Server

**Files:**
- Create: `backend/crates/sober-gateway/src/grpc.rs`
- Modify: `backend/crates/sober-gateway/src/lib.rs`
- Modify: `backend/crates/sober-gateway/src/main.rs`

- [ ] **Step 1: Create `grpc.rs`**

```rust
use std::sync::Arc;

use tonic::{Request, Response, Status};

use crate::bridge::PlatformBridgeRegistry;
use crate::proto::gateway_service_server::GatewayService as GatewayServiceTrait;
use crate::proto::{
    ExternalChannel as ProtoChannel, HealthRequest, HealthResponse, ListChannelsRequest,
    ListChannelsResponse, PlatformStatus, ReloadRequest, ReloadResponse, StatusRequest,
    StatusResponse,
};
use crate::service::GatewayService;

use sober_core::types::PlatformId;

pub struct GatewayGrpcService {
    service: Arc<GatewayService>,
}

impl GatewayGrpcService {
    pub fn new(service: Arc<GatewayService>) -> Self {
        Self { service }
    }
}

#[tonic::async_trait]
impl GatewayServiceTrait for GatewayGrpcService {
    async fn list_channels(
        &self,
        request: Request<ListChannelsRequest>,
    ) -> Result<Response<ListChannelsResponse>, Status> {
        let req = request.into_inner();
        let platform_id = uuid::Uuid::parse_str(&req.platform_id)
            .map(PlatformId::from_uuid)
            .map_err(|_| Status::invalid_argument("invalid platform_id"))?;

        let bridge = self
            .service
            .bridges()
            .get(&platform_id)
            .ok_or_else(|| Status::not_found("platform not connected"))?;

        let channels = bridge
            .list_channels()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ListChannelsResponse {
            channels: channels
                .into_iter()
                .map(|ch| ProtoChannel {
                    id: ch.id,
                    name: ch.name,
                    kind: ch.kind,
                })
                .collect(),
        }))
    }

    async fn reload(
        &self,
        _request: Request<ReloadRequest>,
    ) -> Result<Response<ReloadResponse>, Status> {
        // Reload caches from DB
        self.service
            .load_caches()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ReloadResponse {}))
    }

    async fn status(
        &self,
        _request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let statuses = self.service.bridges().statuses();

        Ok(Response::new(StatusResponse {
            platforms: statuses
                .into_iter()
                .map(|(id, ptype)| PlatformStatus {
                    platform_id: id.to_string(),
                    platform_type: ptype.to_string(),
                    display_name: String::new(), // Could be enriched from cache
                    status: "connected".to_owned(),
                    mapping_count: 0, // Could be enriched from cache
                })
                .collect(),
        }))
    }

    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse { healthy: true }))
    }
}
```

- [ ] **Step 2: Update `lib.rs`**

Add `pub mod grpc;`.

- [ ] **Step 3: Wire gRPC server into `main.rs`**

Update `main.rs` to:
1. Create `PlatformBridgeRegistry`
2. Create `GatewayService`
3. Load caches
4. Create `GatewayGrpcService`
5. Add it to the tonic server
6. Spawn the event loop (receive from `mpsc` channel, call `service.handle_event()`)
7. Subscribe to agent's `SubscribeConversationUpdates` and handle outbound delivery

Replace the placeholder gRPC server section with:

```rust
use std::sync::Arc;
use sober_gateway::bridge::PlatformBridgeRegistry;
use sober_gateway::grpc::GatewayGrpcService;
use sober_gateway::outbound::OutboundBuffer;
use sober_gateway::proto::gateway_service_server::GatewayServiceServer;
use sober_gateway::service::GatewayService;
use sober_gateway::types::GatewayEvent;
use tokio::sync::{mpsc, Mutex};

// ... in main():

// Create bridge registry and gateway service
let bridges = Arc::new(PlatformBridgeRegistry::new());
let gateway_service = Arc::new(GatewayService::new(
    pool.clone(),
    agent_client.clone(),
    bridges.clone(),
));

// Load caches
gateway_service.load_caches().await?;

// Event channel for platform bridges → gateway service
let (event_tx, mut event_rx) = mpsc::channel::<GatewayEvent>(1024);

// TODO: Connect enabled platforms (Discord, etc.) using PlatformConfig from DB + secrets
// For now, platforms are connected manually or on Reload.

// Spawn inbound event processing loop
let svc = gateway_service.clone();
tokio::spawn(async move {
    while let Some(event) = event_rx.recv().await {
        svc.handle_event(event).await;
    }
});

// Spawn outbound delivery loop (subscribe to agent updates)
let outbound_svc = gateway_service.clone();
let outbound_bridges = bridges.clone();
let mut outbound_agent = agent_client.clone();
tokio::spawn(async move {
    let outbound_buffer = Arc::new(Mutex::new(OutboundBuffer::new()));

    // Subscribe with retry
    loop {
        match outbound_agent
            .subscribe_conversation_updates(
                sober_gateway::agent_proto::SubscribeConversationUpdatesRequest {},
            )
            .await
        {
            Ok(response) => {
                let mut stream = response.into_inner();
                info!("subscribed to agent conversation updates");

                while let Ok(Some(update)) = stream.message().await {
                    handle_outbound_update(
                        &update,
                        &outbound_svc,
                        &outbound_bridges,
                        &outbound_buffer,
                    )
                    .await;
                }

                warn!("agent update stream ended, reconnecting...");
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to subscribe to agent updates");
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
});

// gRPC server
let grpc_service = GatewayGrpcService::new(gateway_service.clone());

let listener = UnixListener::bind(socket_path)?;
let stream = UnixListenerStream::new(listener);

info!(socket = %socket_path.display(), "gRPC server listening");

Server::builder()
    .add_service(GatewayServiceServer::new(grpc_service))
    .serve_with_incoming_graceful_shutdown(stream, shutdown_signal())
    .await?;
```

Add the outbound handler function:

```rust
async fn handle_outbound_update(
    update: &sober_gateway::agent_proto::ConversationUpdate,
    service: &GatewayService,
    bridges: &PlatformBridgeRegistry,
    buffer: &Mutex<OutboundBuffer>,
) {
    use sober_gateway::agent_proto::conversation_update::Event;

    let conversation_id = match uuid::Uuid::parse_str(&update.conversation_id) {
        Ok(id) => ConversationId::from_uuid(id),
        Err(_) => return,
    };

    // Quick check: does this conversation have any outbound targets?
    let targets = match service.get_outbound_targets(&conversation_id) {
        Some(t) if !t.is_empty() => t,
        _ => return, // No gateway mapping — discard
    };

    let Some(ref event) = update.event else {
        return;
    };

    match event {
        Event::TextDelta(delta) => {
            buffer.lock().await.append_delta(conversation_id, &delta.text);
        }
        Event::Done(_) => {
            let msg = buffer.lock().await.flush(&conversation_id);
            if let Some(msg) = msg {
                for (platform_id, channel_id) in &targets {
                    if let Some(bridge) = bridges.get(platform_id) {
                        let timer = std::time::Instant::now();
                        match bridge.send_message(channel_id, msg.clone()).await {
                            Ok(()) => {
                                metrics::counter!("sober_gateway_messages_sent_total",
                                    "platform" => platform_id.to_string(),
                                    "status" => "success"
                                ).increment(1);
                                metrics::histogram!(
                                    "sober_gateway_message_delivery_duration_seconds",
                                    "platform" => platform_id.to_string()
                                ).record(timer.elapsed().as_secs_f64());
                            }
                            Err(e) => {
                                metrics::counter!("sober_gateway_messages_sent_total",
                                    "platform" => platform_id.to_string(),
                                    "status" => "error"
                                ).increment(1);
                                tracing::error!(
                                    error = %e,
                                    platform_id = %platform_id,
                                    channel_id = channel_id,
                                    "outbound delivery failed"
                                );
                            }
                        }
                    }
                }
            }
        }
        _ => {} // Ignore other events
    }
}
```

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-gateway -q`
Expected: compiles (check the agent proto for exact field names of `ConversationUpdate`, `TextDelta`, `Done` — adjust the match arms to match the actual generated code)

- [ ] **Step 5: Commit**

```
feat(gateway): wire gRPC server, event loop, and outbound delivery
```

---

## Task 13: Gateway Admin Service in sober-api

**Files:**
- Create: `backend/crates/sober-api/src/services/gateway.rs`
- Modify: `backend/crates/sober-api/src/services/mod.rs`

- [ ] **Step 1: Create `gateway.rs` service**

```rust
use sober_core::error::AppError;
use sober_core::types::{
    ConversationId, CreateChannelMapping, CreatePlatform, CreateUserMapping,
    GatewayChannelMapping, GatewayPlatform, GatewayUserMapping, MappingId, PlatformId,
    UpdatePlatform, UserId, UserMappingId,
};
use sober_db::{PgGatewayMappingRepo, PgGatewayPlatformRepo, PgGatewayUserMappingRepo};
use sqlx::PgPool;
use tracing::instrument;

use crate::guards;

/// Well-known UUID of the gateway bridge bot user.
const BRIDGE_BOT_USER_ID: &str = "01960000-0000-7000-8000-000000000100";

/// Admin service for gateway platform/mapping/user CRUD.
///
/// All operations are DB-backed and work independently of the gateway process.
pub struct GatewayAdminService {
    db: PgPool,
}

impl GatewayAdminService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    // ── Platforms ────────────────────────────────────────────────────────

    #[instrument(level = "debug", skip(self))]
    pub async fn list_platforms(&self) -> Result<Vec<GatewayPlatform>, AppError> {
        let repo = PgGatewayPlatformRepo::new(self.db.clone());
        repo.list(false).await
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn create_platform(
        &self,
        input: CreatePlatform,
    ) -> Result<GatewayPlatform, AppError> {
        let repo = PgGatewayPlatformRepo::new(self.db.clone());
        let id = PlatformId::new();
        repo.create(id, &input).await
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn update_platform(
        &self,
        id: PlatformId,
        input: UpdatePlatform,
    ) -> Result<GatewayPlatform, AppError> {
        let repo = PgGatewayPlatformRepo::new(self.db.clone());
        repo.update(id, &input).await
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn delete_platform(&self, id: PlatformId) -> Result<(), AppError> {
        let repo = PgGatewayPlatformRepo::new(self.db.clone());
        repo.delete(id).await
    }

    // ── Mappings ────────────────────────────────────────────────────────

    #[instrument(level = "debug", skip(self))]
    pub async fn list_mappings(
        &self,
        platform_id: PlatformId,
    ) -> Result<Vec<GatewayChannelMapping>, AppError> {
        let repo = PgGatewayMappingRepo::new(self.db.clone());
        repo.list_by_platform(platform_id).await
    }

    /// Create a channel mapping + add bridge bot as conversation member.
    #[instrument(level = "debug", skip(self))]
    pub async fn create_mapping(
        &self,
        platform_id: PlatformId,
        input: CreateChannelMapping,
    ) -> Result<GatewayChannelMapping, AppError> {
        let bot_uuid =
            uuid::Uuid::parse_str(BRIDGE_BOT_USER_ID).expect("valid bot UUID");
        let bot_user_id = UserId::from_uuid(bot_uuid);
        let mapping_id = MappingId::new();

        let mut tx = self.db.begin().await?;

        // Create the mapping
        let mapping = PgGatewayMappingRepo::create_tx(
            &mut *tx,
            mapping_id,
            platform_id,
            &input,
        )
        .await?;

        // Add bridge bot as conversation member (idempotent via ON CONFLICT)
        sqlx::query(
            "INSERT INTO conversation_users (conversation_id, user_id, role, unread_count) \
             VALUES ($1, $2, 'member', 0) \
             ON CONFLICT (conversation_id, user_id) DO NOTHING"
        )
        .bind(input.conversation_id.as_uuid())
        .bind(bot_user_id.as_uuid())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(mapping)
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn delete_mapping(&self, id: MappingId) -> Result<(), AppError> {
        let repo = PgGatewayMappingRepo::new(self.db.clone());
        repo.delete(id).await
    }

    // ── User Mappings ───────────────────────────────────────────────────

    #[instrument(level = "debug", skip(self))]
    pub async fn list_user_mappings(
        &self,
        platform_id: PlatformId,
    ) -> Result<Vec<GatewayUserMapping>, AppError> {
        let repo = PgGatewayUserMappingRepo::new(self.db.clone());
        repo.list_by_platform(platform_id).await
    }

    /// Create a user mapping + add the Sõber user to all mapped conversations.
    #[instrument(level = "debug", skip(self))]
    pub async fn create_user_mapping(
        &self,
        platform_id: PlatformId,
        input: CreateUserMapping,
    ) -> Result<GatewayUserMapping, AppError> {
        let id = UserMappingId::new();
        let mut tx = self.db.begin().await?;

        let mapping = PgGatewayUserMappingRepo::create_tx(
            &mut *tx,
            id,
            platform_id,
            &input,
        )
        .await?;

        // Add user to all conversations currently mapped for this platform
        sqlx::query(
            "INSERT INTO conversation_users (conversation_id, user_id, role, unread_count) \
             SELECT gcm.conversation_id, $1, 'member', 0 \
             FROM gateway_channel_mappings gcm \
             WHERE gcm.platform_id = $2 \
             ON CONFLICT (conversation_id, user_id) DO NOTHING"
        )
        .bind(input.user_id.as_uuid())
        .bind(platform_id.as_uuid())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(mapping)
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn delete_user_mapping(&self, id: UserMappingId) -> Result<(), AppError> {
        let repo = PgGatewayUserMappingRepo::new(self.db.clone());
        repo.delete(id).await
    }
}
```

- [ ] **Step 2: Register in `services/mod.rs`**

Add `pub mod gateway;`.

- [ ] **Step 3: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: compiles

- [ ] **Step 4: Commit**

```
feat(api): add GatewayAdminService for platform/mapping CRUD
```

---

## Task 14: Gateway Admin Routes in sober-api

**Files:**
- Create: `backend/crates/sober-api/src/routes/gateway.rs`
- Modify: `backend/crates/sober-api/src/routes/mod.rs`
- Modify: `backend/crates/sober-api/src/state.rs`

- [ ] **Step 1: Add `GatewayAdminService` to `AppState`**

In `state.rs`, add to `AppState`:

```rust
pub gateway_admin: Arc<GatewayAdminService>,
```

Add construction in both `from_parts()` and `new()`:

```rust
let gateway_admin = Arc::new(GatewayAdminService::new(db.clone()));
```

And include it in the `Self { ... }` struct construction.

Also add an optional `GatewayClient` type and field for the `ListChannels` proxy. Define the type:

```rust
/// Optional gRPC client for the gateway service.
pub type GatewayClient =
    crate::gateway_proto::gateway_service_client::GatewayServiceClient<Channel>;
```

Add `pub gateway_client: Option<GatewayClient>,` to `AppState`.

For `new()`, attempt to connect but don't fail if the gateway isn't running:

```rust
let gateway_client = match connect_gateway(&config).await {
    Ok(client) => {
        info!("connected to gateway gRPC service");
        Some(client)
    }
    Err(e) => {
        info!(error = %e, "gateway not available (optional)");
        None
    }
};
```

Add a `connect_gateway` function (same UDS pattern as `connect_agent`):

```rust
async fn connect_gateway(config: &AppConfig) -> Result<GatewayClient, AppError> {
    let socket_path = config.gateway.socket_path.clone();
    let channel = Endpoint::try_from("http://[::]:50051")
        .map_err(|e| AppError::Internal(e.into()))?
        .connect_with_connector(service_fn(move |_: Uri| {
            let path = socket_path.clone();
            async move {
                let stream = tokio::net::UnixStream::connect(path).await?;
                Ok::<_, std::io::Error>(TokioIo::new(stream))
            }
        }))
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(GatewayClient::new(channel))
}
```

Also add the gateway proto to sober-api's `build.rs` and `lib.rs`:

In `build.rs`:
```rust
tonic_prost_build::compile_protos("../../proto/sober/gateway/v1/gateway.proto")?;
```

In `src/lib.rs` (or where `proto` modules are declared):
```rust
pub mod gateway_proto {
    tonic::include_proto!("sober.gateway.v1");
}
```

- [ ] **Step 2: Create `routes/gateway.rs`**

```rust
//! Gateway admin routes — platform, mapping, and user mapping CRUD.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use sober_auth::RequireAdmin;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, CreateChannelMapping, CreatePlatform, CreateUserMapping, GatewayChannelMapping,
    GatewayPlatform, GatewayUserMapping, MappingId, PlatformId, UpdatePlatform, UserMappingId,
};

use crate::state::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        // Platforms
        .route("/admin/gateway/platforms", get(list_platforms).post(create_platform))
        .route(
            "/admin/gateway/platforms/{id}",
            get(get_platform).patch(update_platform).delete(delete_platform),
        )
        // Channel mappings
        .route(
            "/admin/gateway/platforms/{id}/channels",
            get(list_available_channels),
        )
        .route(
            "/admin/gateway/platforms/{id}/mappings",
            get(list_mappings).post(create_mapping),
        )
        .route("/admin/gateway/mappings/{id}", delete(delete_mapping))
        // User mappings
        .route(
            "/admin/gateway/platforms/{id}/users",
            get(list_user_mappings).post(create_user_mapping),
        )
        .route(
            "/admin/gateway/user-mappings/{id}",
            delete(delete_user_mapping),
        )
}

// ── Platforms ────────────────────────────────────────────────────────────

async fn list_platforms(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
) -> Result<ApiResponse<Vec<GatewayPlatform>>, AppError> {
    let platforms = state.gateway_admin.list_platforms().await?;
    Ok(ApiResponse::new(platforms))
}

async fn get_platform(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<GatewayPlatform>, AppError> {
    let repo = sober_db::PgGatewayPlatformRepo::new(state.db.clone());
    let platform = sober_core::types::GatewayPlatformRepo::get(&repo, PlatformId::from_uuid(id)).await?;
    Ok(ApiResponse::new(platform))
}

async fn create_platform(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Json(input): Json<CreatePlatform>,
) -> Result<ApiResponse<GatewayPlatform>, AppError> {
    let platform = state.gateway_admin.create_platform(input).await?;
    Ok(ApiResponse::new(platform))
}

async fn update_platform(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<UpdatePlatform>,
) -> Result<ApiResponse<GatewayPlatform>, AppError> {
    let platform = state
        .gateway_admin
        .update_platform(PlatformId::from_uuid(id), input)
        .await?;
    Ok(ApiResponse::new(platform))
}

async fn delete_platform(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state
        .gateway_admin
        .delete_platform(PlatformId::from_uuid(id))
        .await?;
    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}

// ── Channels (from gateway gRPC) ────────────────────────────────────────

async fn list_available_channels(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<Vec<serde_json::Value>>, AppError> {
    let Some(mut client) = state.gateway_client.clone() else {
        return Err(AppError::ServiceUnavailable(
            "gateway service is not available".into(),
        ));
    };

    let response = client
        .list_channels(crate::gateway_proto::ListChannelsRequest {
            platform_id: id.to_string(),
        })
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let channels: Vec<serde_json::Value> = response
        .into_inner()
        .channels
        .into_iter()
        .map(|ch| {
            serde_json::json!({
                "id": ch.id,
                "name": ch.name,
                "kind": ch.kind,
            })
        })
        .collect();

    Ok(ApiResponse::new(channels))
}

// ── Mappings ────────────────────────────────────────────────────────────

async fn list_mappings(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<Vec<GatewayChannelMapping>>, AppError> {
    let mappings = state
        .gateway_admin
        .list_mappings(PlatformId::from_uuid(id))
        .await?;
    Ok(ApiResponse::new(mappings))
}

async fn create_mapping(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<CreateChannelMapping>,
) -> Result<ApiResponse<GatewayChannelMapping>, AppError> {
    let mapping = state
        .gateway_admin
        .create_mapping(PlatformId::from_uuid(id), input)
        .await?;
    Ok(ApiResponse::new(mapping))
}

async fn delete_mapping(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state
        .gateway_admin
        .delete_mapping(MappingId::from_uuid(id))
        .await?;
    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}

// ── User Mappings ───────────────────────────────────────────────────────

async fn list_user_mappings(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<Vec<GatewayUserMapping>>, AppError> {
    let mappings = state
        .gateway_admin
        .list_user_mappings(PlatformId::from_uuid(id))
        .await?;
    Ok(ApiResponse::new(mappings))
}

async fn create_user_mapping(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<CreateUserMapping>,
) -> Result<ApiResponse<GatewayUserMapping>, AppError> {
    let mapping = state
        .gateway_admin
        .create_user_mapping(PlatformId::from_uuid(id), input)
        .await?;
    Ok(ApiResponse::new(mapping))
}

async fn delete_user_mapping(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state
        .gateway_admin
        .delete_user_mapping(UserMappingId::from_uuid(id))
        .await?;
    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}
```

Note: Check if `AppError::ServiceUnavailable` variant exists. If not, add it to `sober-core/src/error.rs` mapping to HTTP 503.

- [ ] **Step 3: Register routes in `routes/mod.rs`**

Add `pub mod gateway;` and merge:
```rust
.merge(gateway::routes())
```

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: compiles

- [ ] **Step 5: Commit**

```
feat(api): add gateway admin routes for platform/mapping/user CRUD
```

---

## Task 15: Frontend Types and API Service

**Files:**
- Create: `frontend/src/lib/types/gateway.ts`
- Create: `frontend/src/lib/services/gateway.ts`

- [ ] **Step 1: Create TypeScript types**

```typescript
export interface GatewayPlatform {
  id: string;
  platform_type: 'discord' | 'telegram' | 'matrix' | 'whatsapp';
  display_name: string;
  is_enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface GatewayChannelMapping {
  id: string;
  platform_id: string;
  external_channel_id: string;
  external_channel_name: string;
  conversation_id: string;
  is_thread: boolean;
  parent_mapping_id: string | null;
  created_at: string;
}

export interface GatewayUserMapping {
  id: string;
  platform_id: string;
  external_user_id: string;
  external_username: string;
  user_id: string;
  created_at: string;
}

export interface ExternalChannel {
  id: string;
  name: string;
  kind: string;
}

export interface CreatePlatformInput {
  platform_type: string;
  display_name: string;
}

export interface UpdatePlatformInput {
  display_name?: string;
  is_enabled?: boolean;
}

export interface CreateMappingInput {
  external_channel_id: string;
  external_channel_name: string;
  conversation_id: string;
}

export interface CreateUserMappingInput {
  external_user_id: string;
  external_username: string;
  user_id: string;
}
```

- [ ] **Step 2: Create API service**

```typescript
import { api } from '$lib/utils/api';
import type {
  GatewayPlatform,
  GatewayChannelMapping,
  GatewayUserMapping,
  ExternalChannel,
  CreatePlatformInput,
  UpdatePlatformInput,
  CreateMappingInput,
  CreateUserMappingInput,
} from '$lib/types/gateway';

const BASE = '/admin/gateway';

export const gatewayService = {
  // Platforms
  listPlatforms: () => api<GatewayPlatform[]>(`${BASE}/platforms`),

  getPlatform: (id: string) => api<GatewayPlatform>(`${BASE}/platforms/${id}`),

  createPlatform: (input: CreatePlatformInput) =>
    api<GatewayPlatform>(`${BASE}/platforms`, {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  updatePlatform: (id: string, input: UpdatePlatformInput) =>
    api<GatewayPlatform>(`${BASE}/platforms/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(input),
    }),

  deletePlatform: (id: string) =>
    api<{ deleted: boolean }>(`${BASE}/platforms/${id}`, { method: 'DELETE' }),

  // Channels (from gateway process)
  listChannels: (platformId: string) =>
    api<ExternalChannel[]>(`${BASE}/platforms/${platformId}/channels`),

  // Mappings
  listMappings: (platformId: string) =>
    api<GatewayChannelMapping[]>(`${BASE}/platforms/${platformId}/mappings`),

  createMapping: (platformId: string, input: CreateMappingInput) =>
    api<GatewayChannelMapping>(`${BASE}/platforms/${platformId}/mappings`, {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  deleteMapping: (id: string) =>
    api<{ deleted: boolean }>(`${BASE}/mappings/${id}`, { method: 'DELETE' }),

  // User mappings
  listUserMappings: (platformId: string) =>
    api<GatewayUserMapping[]>(`${BASE}/platforms/${platformId}/users`),

  createUserMapping: (platformId: string, input: CreateUserMappingInput) =>
    api<GatewayUserMapping>(`${BASE}/platforms/${platformId}/users`, {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  deleteUserMapping: (id: string) =>
    api<{ deleted: boolean }>(`${BASE}/user-mappings/${id}`, { method: 'DELETE' }),
};
```

- [ ] **Step 3: Commit**

```
feat(frontend): add gateway TypeScript types and API service
```

---

## Task 16: Frontend — Gateway Settings Page

**Files:**
- Create: `frontend/src/routes/(app)/settings/gateway/+page.svelte`
- Modify: `frontend/src/routes/(app)/settings/+layout.svelte`

- [ ] **Step 1: Add Gateway tab to settings layout**

In `+layout.svelte`, add a new `RequireRole`-gated tab after the Plugins tab:

```svelte
<RequireRole role="admin">
    <a
        href={resolve('/(app)/settings/gateway')}
        class={tabClass(resolve('/(app)/settings/gateway'))}
    >
        Gateway
    </a>
</RequireRole>
```

- [ ] **Step 2: Create platform list page**

Create `frontend/src/routes/(app)/settings/gateway/+page.svelte`:

```svelte
<script lang="ts">
    import type { GatewayPlatform, CreatePlatformInput } from '$lib/types/gateway';
    import { gatewayService } from '$lib/services/gateway';

    let platforms = $state<GatewayPlatform[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);
    let showAddForm = $state(false);
    let newPlatform = $state<CreatePlatformInput>({
        platform_type: 'discord',
        display_name: '',
    });

    $effect(() => {
        loadPlatforms();
    });

    async function loadPlatforms() {
        loading = true;
        error = null;
        try {
            platforms = await gatewayService.listPlatforms();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to load platforms';
        } finally {
            loading = false;
        }
    }

    async function addPlatform() {
        if (!newPlatform.display_name.trim()) return;
        try {
            await gatewayService.createPlatform(newPlatform);
            showAddForm = false;
            newPlatform = { platform_type: 'discord', display_name: '' };
            await loadPlatforms();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to create platform';
        }
    }

    async function togglePlatform(platform: GatewayPlatform) {
        try {
            await gatewayService.updatePlatform(platform.id, {
                is_enabled: !platform.is_enabled,
            });
            await loadPlatforms();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to update platform';
        }
    }

    async function removePlatform(id: string) {
        try {
            await gatewayService.deletePlatform(id);
            await loadPlatforms();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete platform';
        }
    }

    const platformIcon: Record<string, string> = {
        discord: 'D',
        telegram: 'T',
        matrix: 'M',
        whatsapp: 'W',
    };
</script>

<div class="space-y-6">
    <div class="flex items-center justify-between">
        <div>
            <h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">
                Messaging Gateway
            </h2>
            <p class="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
                Bridge external messaging platforms to Sõber conversations.
            </p>
        </div>
        <button
            onclick={() => (showAddForm = !showAddForm)}
            class="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
        >
            Add Platform
        </button>
    </div>

    {#if error}
        <div
            class="rounded-md border border-red-200 bg-red-50 p-3 text-sm text-red-700 dark:border-red-800 dark:bg-red-900/20 dark:text-red-400"
        >
            {error}
        </div>
    {/if}

    {#if showAddForm}
        <div
            class="rounded-lg border border-zinc-200 p-4 dark:border-zinc-700"
        >
            <h3 class="mb-3 text-sm font-medium text-zinc-900 dark:text-zinc-100">
                New Platform
            </h3>
            <div class="flex gap-3">
                <select
                    bind:value={newPlatform.platform_type}
                    class="rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-100"
                >
                    <option value="discord">Discord</option>
                    <option value="telegram">Telegram</option>
                    <option value="matrix">Matrix</option>
                    <option value="whatsapp">WhatsApp</option>
                </select>
                <input
                    bind:value={newPlatform.display_name}
                    placeholder="Display name"
                    class="flex-1 rounded-md border border-zinc-300 px-3 py-1.5 text-sm dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-100"
                />
                <button
                    onclick={addPlatform}
                    class="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
                >
                    Create
                </button>
            </div>
        </div>
    {/if}

    {#if loading}
        <p class="text-sm text-zinc-500 dark:text-zinc-400">Loading...</p>
    {:else if platforms.length === 0}
        <div
            class="rounded-lg border border-dashed border-zinc-300 p-8 text-center dark:border-zinc-700"
        >
            <p class="text-sm text-zinc-500 dark:text-zinc-400">
                No platforms configured. Add a platform to get started.
            </p>
        </div>
    {:else}
        <div class="space-y-3">
            {#each platforms as platform (platform.id)}
                <a
                    href={`/settings/gateway/${platform.id}`}
                    class="flex items-center justify-between rounded-lg border border-zinc-200 p-4 transition-colors hover:bg-zinc-50 dark:border-zinc-700 dark:hover:bg-zinc-800/50"
                >
                    <div class="flex items-center gap-3">
                        <div
                            class="flex h-8 w-8 items-center justify-center rounded-md bg-zinc-100 text-sm font-bold text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300"
                        >
                            {platformIcon[platform.platform_type] ?? '?'}
                        </div>
                        <div>
                            <p
                                class="text-sm font-medium text-zinc-900 dark:text-zinc-100"
                            >
                                {platform.display_name}
                            </p>
                            <p class="text-xs text-zinc-500 dark:text-zinc-400">
                                {platform.platform_type}
                            </p>
                        </div>
                    </div>
                    <div class="flex items-center gap-3">
                        <span
                            class={[
                                'inline-block h-2 w-2 rounded-full',
                                platform.is_enabled
                                    ? 'bg-emerald-500'
                                    : 'bg-zinc-300 dark:bg-zinc-600',
                            ]}
                        ></span>
                        <button
                            onclick={(e) => {
                                e.preventDefault();
                                togglePlatform(platform);
                            }}
                            class="text-xs text-zinc-500 hover:text-zinc-700 dark:text-zinc-400 dark:hover:text-zinc-200"
                        >
                            {platform.is_enabled ? 'Disable' : 'Enable'}
                        </button>
                        <button
                            onclick={(e) => {
                                e.preventDefault();
                                removePlatform(platform.id);
                            }}
                            class="text-xs text-red-500 hover:text-red-700 dark:text-red-400 dark:hover:text-red-300"
                        >
                            Remove
                        </button>
                    </div>
                </a>
            {/each}
        </div>
    {/if}
</div>
```

- [ ] **Step 3: Verify frontend builds**

Run: `cd frontend && pnpm check`
Expected: no type errors

- [ ] **Step 4: Commit**

```
feat(frontend): add gateway settings platform list page
```

---

## Task 17: Frontend — Platform Detail Page

**Files:**
- Create: `frontend/src/routes/(app)/settings/gateway/[id]/+page.svelte`

- [ ] **Step 1: Create platform detail page**

This page shows channel mappings and user mappings for a specific platform. It includes forms to create new mappings.

```svelte
<script lang="ts">
    import { page } from '$app/stores';
    import type {
        GatewayPlatform,
        GatewayChannelMapping,
        GatewayUserMapping,
        ExternalChannel,
    } from '$lib/types/gateway';
    import { gatewayService } from '$lib/services/gateway';

    const platformId = $derived($page.params.id);

    let platform = $state<GatewayPlatform | null>(null);
    let mappings = $state<GatewayChannelMapping[]>([]);
    let userMappings = $state<GatewayUserMapping[]>([]);
    let availableChannels = $state<ExternalChannel[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);
    let channelsError = $state<string | null>(null);

    // Mapping form
    let showMappingForm = $state(false);
    let selectedChannelId = $state('');
    let selectedChannelName = $state('');
    let conversationId = $state('');

    // User mapping form
    let showUserForm = $state(false);
    let externalUserId = $state('');
    let externalUsername = $state('');
    let soberUserId = $state('');

    $effect(() => {
        loadData();
    });

    async function loadData() {
        loading = true;
        error = null;
        try {
            const [p, m, u] = await Promise.all([
                gatewayService.getPlatform(platformId),
                gatewayService.listMappings(platformId),
                gatewayService.listUserMappings(platformId),
            ]);
            platform = p;
            mappings = m;
            userMappings = u;
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to load platform';
        } finally {
            loading = false;
        }
    }

    async function loadChannels() {
        channelsError = null;
        try {
            availableChannels = await gatewayService.listChannels(platformId);
        } catch (e) {
            channelsError =
                e instanceof Error ? e.message : 'Gateway unavailable — cannot list channels';
            availableChannels = [];
        }
    }

    async function createMapping() {
        if (!selectedChannelId || !conversationId.trim()) return;
        try {
            await gatewayService.createMapping(platformId, {
                external_channel_id: selectedChannelId,
                external_channel_name: selectedChannelName,
                conversation_id: conversationId.trim(),
            });
            showMappingForm = false;
            selectedChannelId = '';
            conversationId = '';
            await loadData();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to create mapping';
        }
    }

    async function removeMapping(id: string) {
        try {
            await gatewayService.deleteMapping(id);
            await loadData();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete mapping';
        }
    }

    async function createUserMapping() {
        if (!externalUserId.trim() || !soberUserId.trim()) return;
        try {
            await gatewayService.createUserMapping(platformId, {
                external_user_id: externalUserId.trim(),
                external_username: externalUsername.trim(),
                user_id: soberUserId.trim(),
            });
            showUserForm = false;
            externalUserId = '';
            externalUsername = '';
            soberUserId = '';
            await loadData();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to create user mapping';
        }
    }

    async function removeUserMapping(id: string) {
        try {
            await gatewayService.deleteUserMapping(id);
            await loadData();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete user mapping';
        }
    }
</script>

{#if loading}
    <p class="text-sm text-zinc-500 dark:text-zinc-400">Loading...</p>
{:else if !platform}
    <p class="text-sm text-red-500">Platform not found</p>
{:else}
    <div class="space-y-8">
        <!-- Header -->
        <div class="flex items-center gap-3">
            <a
                href="/settings/gateway"
                class="text-sm text-zinc-500 hover:text-zinc-700 dark:text-zinc-400 dark:hover:text-zinc-200"
            >
                Gateway
            </a>
            <span class="text-zinc-300 dark:text-zinc-600">/</span>
            <h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">
                {platform.display_name}
            </h2>
            <span
                class="rounded-full bg-zinc-100 px-2 py-0.5 text-xs text-zinc-600 dark:bg-zinc-800 dark:text-zinc-400"
            >
                {platform.platform_type}
            </span>
        </div>

        {#if error}
            <div
                class="rounded-md border border-red-200 bg-red-50 p-3 text-sm text-red-700 dark:border-red-800 dark:bg-red-900/20 dark:text-red-400"
            >
                {error}
            </div>
        {/if}

        <!-- Channel Mappings -->
        <section>
            <div class="mb-3 flex items-center justify-between">
                <h3 class="text-sm font-medium text-zinc-900 dark:text-zinc-100">
                    Channel Mappings
                </h3>
                <button
                    onclick={() => {
                        showMappingForm = !showMappingForm;
                        if (showMappingForm) loadChannels();
                    }}
                    class="text-sm text-zinc-600 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-zinc-100"
                >
                    {showMappingForm ? 'Cancel' : 'Add Mapping'}
                </button>
            </div>

            {#if showMappingForm}
                <div class="mb-4 rounded-lg border border-zinc-200 p-4 dark:border-zinc-700">
                    {#if channelsError}
                        <p class="mb-2 text-sm text-amber-600 dark:text-amber-400">
                            {channelsError}
                        </p>
                    {/if}
                    <div class="flex flex-col gap-3">
                        {#if availableChannels.length > 0}
                            <select
                                bind:value={selectedChannelId}
                                onchange={(e) => {
                                    const ch = availableChannels.find(
                                        (c) => c.id === e.currentTarget.value,
                                    );
                                    selectedChannelName = ch?.name ?? '';
                                }}
                                class="rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-100"
                            >
                                <option value="">Select channel...</option>
                                {#each availableChannels as ch (ch.id)}
                                    <option value={ch.id}>{ch.name}</option>
                                {/each}
                            </select>
                        {:else}
                            <input
                                bind:value={selectedChannelId}
                                placeholder="External channel ID"
                                class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-100"
                            />
                        {/if}
                        <input
                            bind:value={conversationId}
                            placeholder="Sõber conversation ID"
                            class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-100"
                        />
                        <button
                            onclick={createMapping}
                            class="self-end rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
                        >
                            Create Mapping
                        </button>
                    </div>
                </div>
            {/if}

            {#if mappings.length === 0}
                <p class="text-sm text-zinc-500 dark:text-zinc-400">No channel mappings.</p>
            {:else}
                <div class="space-y-2">
                    {#each mappings as mapping (mapping.id)}
                        <div
                            class="flex items-center justify-between rounded-md border border-zinc-200 px-3 py-2 dark:border-zinc-700"
                        >
                            <div>
                                <p class="text-sm text-zinc-900 dark:text-zinc-100">
                                    {mapping.external_channel_name}
                                </p>
                                <p class="text-xs text-zinc-500 dark:text-zinc-400">
                                    {mapping.external_channel_id} → {mapping.conversation_id}
                                </p>
                            </div>
                            <button
                                onclick={() => removeMapping(mapping.id)}
                                class="text-xs text-red-500 hover:text-red-700 dark:text-red-400"
                            >
                                Remove
                            </button>
                        </div>
                    {/each}
                </div>
            {/if}
        </section>

        <!-- User Mappings -->
        <section>
            <div class="mb-3 flex items-center justify-between">
                <h3 class="text-sm font-medium text-zinc-900 dark:text-zinc-100">User Mappings</h3>
                <button
                    onclick={() => (showUserForm = !showUserForm)}
                    class="text-sm text-zinc-600 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-zinc-100"
                >
                    {showUserForm ? 'Cancel' : 'Add User'}
                </button>
            </div>

            {#if showUserForm}
                <div class="mb-4 rounded-lg border border-zinc-200 p-4 dark:border-zinc-700">
                    <div class="flex flex-col gap-3">
                        <input
                            bind:value={externalUserId}
                            placeholder="External user ID"
                            class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-100"
                        />
                        <input
                            bind:value={externalUsername}
                            placeholder="External username"
                            class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-100"
                        />
                        <input
                            bind:value={soberUserId}
                            placeholder="Sõber user ID"
                            class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-100"
                        />
                        <button
                            onclick={createUserMapping}
                            class="self-end rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
                        >
                            Create Mapping
                        </button>
                    </div>
                </div>
            {/if}

            {#if userMappings.length === 0}
                <p class="text-sm text-zinc-500 dark:text-zinc-400">No user mappings.</p>
            {:else}
                <div class="space-y-2">
                    {#each userMappings as um (um.id)}
                        <div
                            class="flex items-center justify-between rounded-md border border-zinc-200 px-3 py-2 dark:border-zinc-700"
                        >
                            <div>
                                <p class="text-sm text-zinc-900 dark:text-zinc-100">
                                    {um.external_username}
                                </p>
                                <p class="text-xs text-zinc-500 dark:text-zinc-400">
                                    {um.external_user_id} → {um.user_id}
                                </p>
                            </div>
                            <button
                                onclick={() => removeUserMapping(um.id)}
                                class="text-xs text-red-500 hover:text-red-700 dark:text-red-400"
                            >
                                Remove
                            </button>
                        </div>
                    {/each}
                </div>
            {/if}
        </section>
    </div>
{/if}
```

- [ ] **Step 2: Verify frontend builds**

Run: `cd frontend && pnpm check`
Expected: no type errors

- [ ] **Step 3: Commit**

```
feat(frontend): add platform detail page with mapping management
```

---

## Task 18: Docker and Infrastructure

**Files:**
- Create: `infra/docker/Dockerfile.gateway`
- Modify: `docker-compose.yml`
- Modify: `docker-bake.hcl`
- Modify: `infra/docker/Dockerfile.ci`

- [ ] **Step 1: Create `Dockerfile.gateway`**

```dockerfile
FROM rust:latest AS builder
RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY backend/ backend/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/build/backend/target,id=sober-gateway-target \
    cd backend && cargo build --release -p sober-gateway \
    && cp target/release/sober-gateway /usr/local/bin/sober-gateway

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --no-create-home --shell /usr/sbin/nologin sober
COPY --from=builder /usr/local/bin/sober-gateway /usr/local/bin/sober-gateway
RUN mkdir -p /run/sober && chown sober:sober /run/sober
USER sober
ENTRYPOINT ["sober-gateway"]
```

- [ ] **Step 2: Add to `docker-compose.yml`**

Add after `sober-scheduler` service:

```yaml
  sober-gateway:
    build:
      context: .
      dockerfile: infra/docker/Dockerfile.gateway
    ports:
      - "9102:9102"
    environment:
      <<: [*common-env, *crypto-env, *otel-env]
      SOBER_GATEWAY_SOCKET_PATH: /run/sober/gateway.sock
      SOBER_GATEWAY_AGENT_SOCKET_PATH: /run/sober/agent.sock
      SOBER_GATEWAY_METRICS_PORT: "9102"
      OTEL_SERVICE_NAME: sober-gateway
    volumes:
      - sober_sockets:/run/sober
    depends_on:
      sober-agent:
        condition: service_healthy
```

- [ ] **Step 3: Add to `docker-bake.hcl`**

Add `"sober-gateway"` to the `default` group targets list, and add:

```hcl
target "sober-gateway" {
  inherits = ["_common"]
  target   = "sober-gateway"
  tags     = ["${REGISTRY}/sober-gateway:${TAG}"]
}
```

- [ ] **Step 4: Add to `Dockerfile.ci`**

Add a gateway runtime stage after the scheduler stage:

```dockerfile
# ============================================================
# Runtime: sober-gateway
# ============================================================
FROM debian:trixie-slim AS sober-gateway
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --no-create-home --shell /usr/sbin/nologin sober
COPY --from=builder /usr/local/bin/sober-gateway /usr/local/bin/sober-gateway
RUN mkdir -p /run/sober && chown sober:sober /run/sober
USER sober
ENTRYPOINT ["sober-gateway"]
```

Also add `&& cp target/release/sober-gateway /usr/local/bin/sober-gateway` to the builder stage's `cargo build` and `cp` commands.

- [ ] **Step 5: Commit**

```
chore(infra): add gateway Docker and compose configuration
```

---

## Task 19: Metrics and Observability

**Files:**
- Create: `backend/crates/sober-gateway/metrics.toml`

- [ ] **Step 1: Create `metrics.toml`**

```toml
[group.gateway_messages]
name = "Message Gateway"
description = "Inbound and outbound message handling."

[[group.gateway_messages.metrics]]
name = "sober_gateway_messages_received_total"
type = "counter"
description = "Total inbound platform messages processed."
labels = ["platform", "status"]

[[group.gateway_messages.metrics]]
name = "sober_gateway_messages_sent_total"
type = "counter"
description = "Total outbound messages delivered to platforms."
labels = ["platform", "status"]

[[group.gateway_messages.metrics]]
name = "sober_gateway_message_handle_duration_seconds"
type = "histogram"
description = "Inbound message processing latency (gateway → agent RPC)."
labels = ["platform"]

[[group.gateway_messages.metrics]]
name = "sober_gateway_message_delivery_duration_seconds"
type = "histogram"
description = "Outbound message delivery latency (gateway → platform)."
labels = ["platform"]

[[group.gateway_messages.metrics]]
name = "sober_gateway_unmapped_messages_total"
type = "counter"
description = "Messages dropped due to unmapped channel."
labels = ["platform"]

[[group.gateway_messages.metrics]]
name = "sober_gateway_buffer_flush_size_bytes"
type = "histogram"
description = "Size of buffered content flushed to platforms."
labels = ["platform"]

[group.gateway_platforms]
name = "Platform Connections"
description = "Platform bridge connection state."

[[group.gateway_platforms.metrics]]
name = "sober_gateway_platform_connections"
type = "gauge"
description = "Active platform connections by status."
labels = ["platform", "status"]

[[group.gateway_platforms.metrics]]
name = "sober_gateway_platform_errors_total"
type = "counter"
description = "Platform SDK errors."
labels = ["platform", "error_type"]

[alerts]

[[alerts.rules]]
name = "GatewayHighErrorRate"
expr = "rate(sober_gateway_platform_errors_total[5m]) > 5"
for = "5m"
severity = "warning"
description = "Gateway platform error rate exceeds 5/min."

[[alerts.rules]]
name = "GatewayPlatformDisconnected"
expr = "sober_gateway_platform_connections{status=\"disconnected\"} > 0"
for = "5m"
severity = "warning"
description = "A gateway platform has been disconnected for >5 minutes."

[[alerts.rules]]
name = "GatewayHighP95Latency"
expr = "histogram_quantile(0.95, rate(sober_gateway_message_handle_duration_seconds_bucket[5m])) > 2.0"
for = "5m"
severity = "warning"
description = "Gateway inbound message p95 latency exceeds 2 seconds."
```

- [ ] **Step 2: Commit**

```
feat(gateway): add metrics.toml with metric declarations and alerts
```

---

## Task 20: Documentation Updates

**Files:**
- Modify: `ARCHITECTURE.md`

- [ ] **Step 1: Update system architecture diagram**

Add `sober-gateway` to the system architecture diagram in `ARCHITECTURE.md`:
- Add it as a new box connected to `sober-agent` via gRPC/UDS
- Add Discord/Telegram/Matrix/WhatsApp as external connections
- Show the gRPC socket `/run/sober/gateway.sock`

- [ ] **Step 2: Update Crate Map**

Add to the crate table:

```
| `sober-gateway` | **Binary crate (gRPC server process).** External messaging platform bridge. Connects to Discord/Telegram/Matrix/WhatsApp via SDK libraries, routes inbound messages to `sober-agent` via gRPC/UDS, delivers agent responses back to platforms. In-memory mapping caches for O(1) routing. Exposes `ListChannels`/`Reload`/`Status` RPCs for `sober-api` admin integration. |
```

- [ ] **Step 3: Update Deployment table**

Add gateway to the process table:

```
| `sober-gateway` | External platform bridge | `/run/sober/gateway.sock` |
```

- [ ] **Step 4: Update Docker Image Builds table**

Add gateway to both dev and CI build descriptions.

- [ ] **Step 5: Commit**

```
docs(arch): add sober-gateway to architecture documentation
```

---

## Task 21: sqlx Prepare and Final Verification

- [ ] **Step 1: Run sqlx prepare**

Run: `cd backend && cargo sqlx prepare --workspace -q`
Expected: `.sqlx/` directory updated with query metadata

- [ ] **Step 2: Full workspace build**

Run: `cd backend && cargo build --workspace -q`
Expected: all crates compile

- [ ] **Step 3: Clippy**

Run: `cd backend && cargo clippy --workspace -q -- -D warnings`
Expected: no warnings

- [ ] **Step 4: Format**

Run: `cd backend && cargo fmt --check -q`
Expected: no formatting issues

- [ ] **Step 5: Frontend check**

Run: `cd frontend && pnpm check`
Expected: no errors

- [ ] **Step 6: Run existing tests**

Run: `cd backend && cargo test --workspace -q`
Expected: all existing tests pass (gateway has no integration tests yet — those come in a follow-up)

- [ ] **Step 7: Docker build**

Run: `docker compose up -d --build --quiet-pull 2>&1 | tail -15`
Expected: all services start, including sober-gateway

- [ ] **Step 8: Commit sqlx metadata**

```
chore(db): update sqlx offline metadata for gateway queries
```

---

## Task 22: Move Plan to Active

- [ ] **Step 1: Move plan folder**

```bash
git mv docs/plans/pending/045-gateway docs/plans/active/045-gateway
```

- [ ] **Step 2: Commit**

```
docs(plans): move #045 gateway to active
```
