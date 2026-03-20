# #019 Plan A: Unified Plugin Registry Foundation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the unified plugin registry — core types, database schema, repository implementation, manifest parsing, capability types, audit pipeline, and registry CRUD — so all three plugin kinds (MCP, Skill, WASM) share one data model and lifecycle.

**Architecture:** Plugin types live in `sober-core`, PostgreSQL storage in `sober-db`, and the `sober-plugin` crate provides manifest parsing, capability types, audit validation, and the registry API. This plan does NOT wire the registry into the agent or API (that's Plan C). It does NOT implement WASM execution (that's Plan B). It delivers a testable foundation.

**Tech Stack:** Rust, sqlx, serde, toml, thiserror, tracing. No Extism dependency yet (Plan B).

**Design doc:** `docs/plans/pending/019-sober-plugin/design.md`

**Subsequent plans:**
- Plan B: WASM Plugin Runtime (Extism, capabilities host functions, sober-pdk)
- Plan C: Integration & Self-Evolution (agent wiring, API, frontend, sober-plugin-gen)

---

## File Structure

### New files

| File | Responsibility |
|------|---------------|
| `backend/crates/sober-plugin/Cargo.toml` | Crate manifest (replaces stub) |
| `backend/crates/sober-plugin/src/lib.rs` | Module declarations, re-exports |
| `backend/crates/sober-plugin/src/error.rs` | `PluginError` enum |
| `backend/crates/sober-plugin/src/capability.rs` | `Capability` enum, `Cap<T>`, `CapabilitiesConfig` |
| `backend/crates/sober-plugin/src/manifest.rs` | `PluginManifest` parsing from TOML |
| `backend/crates/sober-plugin/src/audit.rs` | `AuditPipeline`, `AuditReport`, `AuditVerdict` |
| `backend/crates/sober-plugin/src/registry.rs` | `PluginRegistry` CRUD operations |
| `backend/migrations/YYYYMMDD000001_create_plugins.sql` | plugins, plugin_audit_logs, plugin_kv_data tables |
| `backend/crates/sober-db/src/repos/plugin.rs` | `PgPluginRepo` implementation |

### Modified files

| File | Change |
|------|--------|
| `backend/Cargo.toml` | Remove `sober-plugin` from `exclude` |
| `backend/crates/sober-core/src/types/ids.rs` | Add `define_id!(PluginId)` |
| `backend/crates/sober-core/src/types/enums.rs` | Add `PluginKind`, `PluginOrigin`, `PluginScope`, `PluginStatus` |
| `backend/crates/sober-core/src/types/domain.rs` | Add `Plugin`, `PluginAuditLog` domain types |
| `backend/crates/sober-core/src/types/input.rs` | Add `CreatePlugin`, `UpdatePluginStatus`, `CreatePluginAuditLog`, `PluginFilter` |
| `backend/crates/sober-core/src/types/repo.rs` | Add `PluginRepo` trait |
| `backend/crates/sober-core/src/types/mod.rs` | Re-export new types |
| `backend/crates/sober-db/src/repos/mod.rs` | Add `plugin` module |
| `backend/crates/sober-db/src/rows.rs` | Add `PluginRow`, `PluginAuditLogRow` |

---

## Task 1: Add PluginId to sober-core

**Files:**
- Modify: `backend/crates/sober-core/src/types/ids.rs`

- [ ] **Step 1: Add PluginId**

Add after the last `define_id!` call in `ids.rs`:

```rust
define_id!(PluginId);
```

- [ ] **Step 2: Verify**

Run: `cargo build -p sober-core -q`

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-core/src/types/ids.rs
git commit -m "feat(core): add PluginId newtype"
```

---

## Task 2: Add plugin enums to sober-core

**Files:**
- Modify: `backend/crates/sober-core/src/types/enums.rs`

- [ ] **Step 1: Add four plugin enums**

Add after the last enum in `enums.rs`. Follow the existing pattern exactly
(see `UserStatus`, `JobStatus` etc. for reference):

```rust
/// What kind of plugin this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(feature = "postgres", sqlx(type_name = "plugin_kind", rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    Mcp,
    Skill,
    Wasm,
}

/// Where the plugin came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(feature = "postgres", sqlx(type_name = "plugin_origin", rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum PluginOrigin {
    System,
    Agent,
    User,
}

/// Visibility scope of a plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(feature = "postgres", sqlx(type_name = "plugin_scope", rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum PluginScope {
    System,
    User,
    Workspace,
}

/// Lifecycle status of a plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(feature = "postgres", sqlx(type_name = "plugin_status", rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum PluginStatus {
    Enabled,
    Disabled,
    Failed,
}
```

- [ ] **Step 2: Verify**

Run: `cargo build -p sober-core -q`

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-core/src/types/enums.rs
git commit -m "feat(core): add PluginKind, PluginOrigin, PluginScope, PluginStatus enums"
```

---

## Task 3: Add plugin domain types

**Files:**
- Modify: `backend/crates/sober-core/src/types/domain.rs`

- [ ] **Step 1: Add Plugin and PluginAuditLog domain types**

Follow the existing pattern (see `McpServerConfig`, `Job`, etc.):

```rust
/// A registered plugin (MCP server, skill, or WASM module).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    pub id: PluginId,
    pub name: String,
    pub kind: PluginKind,
    pub version: Option<String>,
    pub description: Option<String>,
    pub origin: PluginOrigin,
    pub scope: PluginScope,
    pub owner_id: Option<UserId>,
    pub workspace_id: Option<WorkspaceId>,
    pub status: PluginStatus,
    pub config: serde_json::Value,
    pub installed_by: Option<UserId>,
    pub installed_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// An audit log entry for a plugin install attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAuditLog {
    pub id: uuid::Uuid,
    pub plugin_id: Option<PluginId>,
    pub plugin_name: String,
    pub kind: PluginKind,
    pub origin: PluginOrigin,
    pub stages: serde_json::Value,
    pub verdict: String,
    pub rejection_reason: Option<String>,
    pub audited_at: DateTime<Utc>,
    pub audited_by: Option<UserId>,
}
```

- [ ] **Step 2: Add necessary imports to domain.rs**

Add `PluginId` and plugin enum imports to the use block at the top of `domain.rs`.

- [ ] **Step 3: Verify**

Run: `cargo build -p sober-core -q`

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-core/src/types/domain.rs
git commit -m "feat(core): add Plugin and PluginAuditLog domain types"
```

---

## Task 4: Add plugin input types

**Files:**
- Modify: `backend/crates/sober-core/src/types/input.rs`

- [ ] **Step 1: Add input types**

```rust
/// Input for creating a new plugin record.
#[derive(Debug, Clone)]
pub struct CreatePlugin {
    pub name: String,
    pub kind: PluginKind,
    pub version: Option<String>,
    pub description: Option<String>,
    pub origin: PluginOrigin,
    pub scope: PluginScope,
    pub owner_id: Option<UserId>,
    pub workspace_id: Option<WorkspaceId>,
    pub status: PluginStatus,
    pub config: serde_json::Value,
    pub installed_by: Option<UserId>,
}

/// Input for creating a plugin audit log entry.
#[derive(Debug, Clone)]
pub struct CreatePluginAuditLog {
    pub plugin_id: Option<PluginId>,
    pub plugin_name: String,
    pub kind: PluginKind,
    pub origin: PluginOrigin,
    pub stages: serde_json::Value,
    pub verdict: String,
    pub rejection_reason: Option<String>,
    pub audited_by: Option<UserId>,
}

/// Filter for listing plugins.
#[derive(Debug, Clone, Default)]
pub struct PluginFilter {
    pub kind: Option<PluginKind>,
    pub scope: Option<PluginScope>,
    pub owner_id: Option<UserId>,
    pub workspace_id: Option<WorkspaceId>,
    pub status: Option<PluginStatus>,
}
```

- [ ] **Step 2: Verify**

Run: `cargo build -p sober-core -q`

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-core/src/types/input.rs
git commit -m "feat(core): add plugin input types (CreatePlugin, PluginFilter)"
```

---

## Task 5: Add PluginRepo trait

**Files:**
- Modify: `backend/crates/sober-core/src/types/repo.rs`

- [ ] **Step 1: Add PluginRepo trait**

Follow the RPITIT pattern (see `McpServerRepo`, `JobRepo`):

```rust
/// Plugin registry operations.
pub trait PluginRepo: Send + Sync {
    /// Creates a new plugin record.
    fn create(
        &self,
        input: CreatePlugin,
    ) -> impl Future<Output = Result<Plugin, AppError>> + Send;

    /// Finds a plugin by ID.
    fn get_by_id(
        &self,
        id: PluginId,
    ) -> impl Future<Output = Result<Plugin, AppError>> + Send;

    /// Finds a plugin by name, scope, and owner.
    fn get_by_name(
        &self,
        name: &str,
        scope: PluginScope,
        owner_id: Option<UserId>,
        workspace_id: Option<WorkspaceId>,
    ) -> impl Future<Output = Result<Option<Plugin>, AppError>> + Send;

    /// Lists plugins with optional filters.
    fn list(
        &self,
        filter: PluginFilter,
    ) -> impl Future<Output = Result<Vec<Plugin>, AppError>> + Send;

    /// Updates a plugin's status.
    fn update_status(
        &self,
        id: PluginId,
        status: PluginStatus,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Updates a plugin's config.
    fn update_config(
        &self,
        id: PluginId,
        config: serde_json::Value,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Deletes a plugin by ID.
    fn delete(
        &self,
        id: PluginId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Creates an audit log entry.
    fn create_audit_log(
        &self,
        input: CreatePluginAuditLog,
    ) -> impl Future<Output = Result<PluginAuditLog, AppError>> + Send;

    /// Lists audit logs for a plugin.
    fn list_audit_logs(
        &self,
        plugin_id: PluginId,
    ) -> impl Future<Output = Result<Vec<PluginAuditLog>, AppError>> + Send;

    /// Gets the KV data blob for a plugin.
    fn get_kv_data(
        &self,
        plugin_id: PluginId,
    ) -> impl Future<Output = Result<serde_json::Value, AppError>> + Send;

    /// Upserts the KV data blob for a plugin.
    fn set_kv_data(
        &self,
        plugin_id: PluginId,
        data: serde_json::Value,
    ) -> impl Future<Output = Result<(), AppError>> + Send;
}
```

- [ ] **Step 2: Add imports**

Add `PluginId`, `PluginScope`, `PluginStatus`, `Plugin`, `PluginAuditLog`,
`CreatePlugin`, `CreatePluginAuditLog`, `PluginFilter` to the import block.

- [ ] **Step 3: Verify**

Run: `cargo build -p sober-core -q`

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-core/src/types/repo.rs
git commit -m "feat(core): add PluginRepo trait with CRUD, audit logs, and KV data"
```

---

## Task 6: Re-export new types from mod.rs

**Files:**
- Modify: `backend/crates/sober-core/src/types/mod.rs`

- [ ] **Step 1: Add re-exports**

Ensure the new types are re-exported from the `types` module. Follow the
existing pattern in `mod.rs` — if it re-exports via `pub use`, add the
new items. If it re-exports via `pub mod`, they're already visible.

- [ ] **Step 2: Verify**

Run: `cargo build -p sober-core -q`

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-core/src/types/mod.rs
git commit -m "feat(core): re-export plugin types from types module"
```

---

## Task 7: Database migration

**Files:**
- Create: `backend/migrations/YYYYMMDD000001_create_plugins.sql`

Use today's date for the timestamp prefix.

- [ ] **Step 1: Write the migration**

```sql
-- Plugin system tables
-- Part of plan #019: Unified Plugin System

CREATE TYPE plugin_kind AS ENUM ('mcp', 'skill', 'wasm');
CREATE TYPE plugin_origin AS ENUM ('system', 'agent', 'user');
CREATE TYPE plugin_scope AS ENUM ('system', 'user', 'workspace');
CREATE TYPE plugin_status AS ENUM ('enabled', 'disabled', 'failed');

CREATE TABLE plugins (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name           TEXT NOT NULL,
    kind           plugin_kind NOT NULL,
    version        TEXT,
    description    TEXT,
    origin         plugin_origin NOT NULL DEFAULT 'user',
    scope          plugin_scope NOT NULL,
    owner_id       UUID REFERENCES users(id),
    workspace_id   UUID REFERENCES workspaces(id),
    status         plugin_status NOT NULL DEFAULT 'enabled',
    config         JSONB NOT NULL DEFAULT '{}',
    installed_by   UUID REFERENCES users(id),
    installed_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE(name, scope,
           COALESCE(owner_id, '00000000-0000-0000-0000-000000000000'),
           COALESCE(workspace_id, '00000000-0000-0000-0000-000000000000'))
);

CREATE INDEX idx_plugins_owner ON plugins (owner_id) WHERE owner_id IS NOT NULL;
CREATE INDEX idx_plugins_workspace ON plugins (workspace_id) WHERE workspace_id IS NOT NULL;
CREATE INDEX idx_plugins_kind_status ON plugins (kind, status);

CREATE TABLE plugin_audit_logs (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plugin_id        UUID REFERENCES plugins(id) ON DELETE SET NULL,
    plugin_name      TEXT NOT NULL,
    kind             plugin_kind NOT NULL,
    origin           plugin_origin NOT NULL,
    stages           JSONB NOT NULL,
    verdict          TEXT NOT NULL,
    rejection_reason TEXT,
    audited_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    audited_by       UUID REFERENCES users(id)
);

CREATE INDEX idx_plugin_audit_logs_plugin ON plugin_audit_logs (plugin_id)
    WHERE plugin_id IS NOT NULL;

CREATE TABLE plugin_kv_data (
    plugin_id  UUID PRIMARY KEY REFERENCES plugins(id) ON DELETE CASCADE,
    data       JSONB NOT NULL DEFAULT '{}',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

- [ ] **Step 2: Verify migration compiles**

Run: `cd backend && sqlx migrate info` (requires Docker/PostgreSQL running)

If Docker is not running, verify the SQL is syntactically correct by reading it.

- [ ] **Step 3: Commit**

```bash
git add backend/migrations/
git commit -m "feat(db): add plugins, plugin_audit_logs, plugin_kv_data tables"
```

---

## Task 8: Add row types to sober-db

**Files:**
- Modify: `backend/crates/sober-db/src/rows.rs`

- [ ] **Step 1: Add PluginRow and PluginAuditLogRow**

Follow the existing pattern (see `UserRow`, `McpServerRow`):

```rust
#[derive(sqlx::FromRow)]
pub(crate) struct PluginRow {
    pub id: Uuid,
    pub name: String,
    pub kind: PluginKind,
    pub version: Option<String>,
    pub description: Option<String>,
    pub origin: PluginOrigin,
    pub scope: PluginScope,
    pub owner_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub status: PluginStatus,
    pub config: serde_json::Value,
    pub installed_by: Option<Uuid>,
    pub installed_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<PluginRow> for Plugin {
    fn from(row: PluginRow) -> Self {
        Self {
            id: PluginId::from_uuid(row.id),
            name: row.name,
            kind: row.kind,
            version: row.version,
            description: row.description,
            origin: row.origin,
            scope: row.scope,
            owner_id: row.owner_id.map(UserId::from_uuid),
            workspace_id: row.workspace_id.map(WorkspaceId::from_uuid),
            status: row.status,
            config: row.config,
            installed_by: row.installed_by.map(UserId::from_uuid),
            installed_at: row.installed_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct PluginAuditLogRow {
    pub id: Uuid,
    pub plugin_id: Option<Uuid>,
    pub plugin_name: String,
    pub kind: PluginKind,
    pub origin: PluginOrigin,
    pub stages: serde_json::Value,
    pub verdict: String,
    pub rejection_reason: Option<String>,
    pub audited_at: DateTime<Utc>,
    pub audited_by: Option<Uuid>,
}

impl From<PluginAuditLogRow> for PluginAuditLog {
    fn from(row: PluginAuditLogRow) -> Self {
        Self {
            id: row.id,
            plugin_id: row.plugin_id.map(PluginId::from_uuid),
            plugin_name: row.plugin_name,
            kind: row.kind,
            origin: row.origin,
            stages: row.stages,
            verdict: row.verdict,
            rejection_reason: row.rejection_reason,
            audited_at: row.audited_at,
            audited_by: row.audited_by.map(UserId::from_uuid),
        }
    }
}
```

- [ ] **Step 2: Add imports**

Add `Plugin`, `PluginAuditLog`, `PluginId`, `PluginKind`, `PluginOrigin`,
`PluginScope`, `PluginStatus` to the imports in `rows.rs`.

- [ ] **Step 3: Verify**

Run: `cargo build -p sober-db -q`

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-db/src/rows.rs
git commit -m "feat(db): add PluginRow and PluginAuditLogRow with From impls"
```

---

## Task 9: Implement PgPluginRepo

**Files:**
- Create: `backend/crates/sober-db/src/repos/plugin.rs`
- Modify: `backend/crates/sober-db/src/repos/mod.rs`

- [ ] **Step 1: Create plugin.rs**

Follow the pattern from `mcp_servers.rs`:

```rust
//! PostgreSQL implementation of [`PluginRepo`].

use sober_core::error::AppError;
use sober_core::types::*;
use sqlx::PgPool;

use crate::rows::{PluginAuditLogRow, PluginRow};

/// PostgreSQL-backed plugin repository.
#[derive(Debug, Clone)]
pub struct PgPluginRepo {
    pool: PgPool,
}

impl PgPluginRepo {
    /// Creates a new repository using the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl PluginRepo for PgPluginRepo {
    async fn create(&self, input: CreatePlugin) -> Result<Plugin, AppError> {
        let row = sqlx::query_as::<_, PluginRow>(
            r#"
            INSERT INTO plugins (name, kind, version, description, origin, scope,
                                 owner_id, workspace_id, status, config, installed_by)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING *
            "#,
        )
        .bind(&input.name)
        .bind(input.kind)
        .bind(&input.version)
        .bind(&input.description)
        .bind(input.origin)
        .bind(input.scope)
        .bind(input.owner_id.map(|id| id.as_uuid()))
        .bind(input.workspace_id.map(|id| id.as_uuid()))
        .bind(input.status)
        .bind(&input.config)
        .bind(input.installed_by.map(|id| id.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        Ok(row.into())
    }

    async fn get_by_id(&self, id: PluginId) -> Result<Plugin, AppError> {
        let row = sqlx::query_as::<_, PluginRow>(
            "SELECT * FROM plugins WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound(format!("plugin {id}")))?;
        Ok(row.into())
    }

    async fn get_by_name(
        &self,
        name: &str,
        scope: PluginScope,
        owner_id: Option<UserId>,
        workspace_id: Option<WorkspaceId>,
    ) -> Result<Option<Plugin>, AppError> {
        let row = sqlx::query_as::<_, PluginRow>(
            r#"
            SELECT * FROM plugins
            WHERE name = $1 AND scope = $2
              AND COALESCE(owner_id, '00000000-0000-0000-0000-000000000000')
                = COALESCE($3, '00000000-0000-0000-0000-000000000000')
              AND COALESCE(workspace_id, '00000000-0000-0000-0000-000000000000')
                = COALESCE($4, '00000000-0000-0000-0000-000000000000')
            "#,
        )
        .bind(name)
        .bind(scope)
        .bind(owner_id.map(|id| id.as_uuid()))
        .bind(workspace_id.map(|id| id.as_uuid()))
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        Ok(row.map(Into::into))
    }

    async fn list(&self, filter: PluginFilter) -> Result<Vec<Plugin>, AppError> {
        // Build dynamic query with optional filters.
        // Using a simple approach: always-true WHERE clause + optional ANDs.
        let rows = sqlx::query_as::<_, PluginRow>(
            r#"
            SELECT * FROM plugins
            WHERE ($1::plugin_kind IS NULL OR kind = $1)
              AND ($2::plugin_scope IS NULL OR scope = $2)
              AND ($3::UUID IS NULL OR owner_id = $3)
              AND ($4::UUID IS NULL OR workspace_id = $4)
              AND ($5::plugin_status IS NULL OR status = $5)
            ORDER BY installed_at DESC
            "#,
        )
        .bind(filter.kind)
        .bind(filter.scope)
        .bind(filter.owner_id.map(|id| id.as_uuid()))
        .bind(filter.workspace_id.map(|id| id.as_uuid()))
        .bind(filter.status)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn update_status(
        &self,
        id: PluginId,
        status: PluginStatus,
    ) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE plugins SET status = $1, updated_at = now() WHERE id = $2",
        )
        .bind(status)
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("plugin {id}")));
        }
        Ok(())
    }

    async fn update_config(
        &self,
        id: PluginId,
        config: serde_json::Value,
    ) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE plugins SET config = $1, updated_at = now() WHERE id = $2",
        )
        .bind(&config)
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("plugin {id}")));
        }
        Ok(())
    }

    async fn delete(&self, id: PluginId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM plugins WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("plugin {id}")));
        }
        Ok(())
    }

    async fn create_audit_log(
        &self,
        input: CreatePluginAuditLog,
    ) -> Result<PluginAuditLog, AppError> {
        let row = sqlx::query_as::<_, PluginAuditLogRow>(
            r#"
            INSERT INTO plugin_audit_logs
                (plugin_id, plugin_name, kind, origin, stages, verdict,
                 rejection_reason, audited_by)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
            "#,
        )
        .bind(input.plugin_id.map(|id| id.as_uuid()))
        .bind(&input.plugin_name)
        .bind(input.kind)
        .bind(input.origin)
        .bind(&input.stages)
        .bind(&input.verdict)
        .bind(&input.rejection_reason)
        .bind(input.audited_by.map(|id| id.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        Ok(row.into())
    }

    async fn list_audit_logs(
        &self,
        plugin_id: PluginId,
    ) -> Result<Vec<PluginAuditLog>, AppError> {
        let rows = sqlx::query_as::<_, PluginAuditLogRow>(
            "SELECT * FROM plugin_audit_logs WHERE plugin_id = $1 ORDER BY audited_at DESC",
        )
        .bind(plugin_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_kv_data(
        &self,
        plugin_id: PluginId,
    ) -> Result<serde_json::Value, AppError> {
        let row: Option<(serde_json::Value,)> = sqlx::query_as(
            "SELECT data FROM plugin_kv_data WHERE plugin_id = $1",
        )
        .bind(plugin_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        Ok(row.map(|r| r.0).unwrap_or_else(|| serde_json::json!({})))
    }

    async fn set_kv_data(
        &self,
        plugin_id: PluginId,
        data: serde_json::Value,
    ) -> Result<(), AppError> {
        sqlx::query(
            r#"
            INSERT INTO plugin_kv_data (plugin_id, data, updated_at)
            VALUES ($1, $2, now())
            ON CONFLICT (plugin_id) DO UPDATE
            SET data = $2, updated_at = now()
            "#,
        )
        .bind(plugin_id.as_uuid())
        .bind(&data)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        Ok(())
    }
}
```

- [ ] **Step 2: Register module in mod.rs**

Add `pub mod plugin;` and `pub use plugin::PgPluginRepo;` to
`backend/crates/sober-db/src/repos/mod.rs`.

- [ ] **Step 3: Verify**

Run: `cargo build -p sober-db -q`

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-db/src/repos/
git commit -m "feat(db): implement PgPluginRepo with CRUD, audit logs, and KV data"
```

---

## Task 10: Scaffold sober-plugin crate

**Files:**
- Modify: `backend/crates/sober-plugin/Cargo.toml`
- Modify: `backend/crates/sober-plugin/src/lib.rs`
- Modify: `backend/Cargo.toml` (remove from exclude)

- [ ] **Step 1: Update Cargo.toml**

Replace the stub `Cargo.toml`:

```toml
[package]
name = "sober-plugin"
version = "0.1.0"
edition.workspace = true

[dependencies]
sober-core = { path = "../sober-core" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
tracing = "0.1"
thiserror = "2"
uuid = { version = "1", features = ["v7"] }
chrono = { version = "0.4", features = ["serde"] }

[dev-dependencies]
pretty_assertions = "1"
```

- [ ] **Step 2: Update lib.rs with module declarations**

```rust
//! Unified plugin system — registry, audit, manifest, and capability types.
//!
//! Manages all three plugin kinds (MCP, Skill, WASM) under one registry
//! with type-aware lifecycle and audit pipeline.

pub mod audit;
pub mod capability;
pub mod error;
pub mod manifest;
pub mod registry;

pub use audit::{AuditPipeline, AuditReport, AuditVerdict, StageResult};
pub use capability::{Cap, CapabilitiesConfig, Capability};
pub use error::PluginError;
pub use manifest::PluginManifest;
pub use registry::PluginRegistry;
```

- [ ] **Step 3: Remove sober-plugin from workspace exclude**

In `backend/Cargo.toml`, remove `"crates/sober-plugin"` from the `exclude` list.

- [ ] **Step 4: Create empty module files**

Create empty files so the crate compiles:
- `backend/crates/sober-plugin/src/error.rs`
- `backend/crates/sober-plugin/src/capability.rs`
- `backend/crates/sober-plugin/src/manifest.rs`
- `backend/crates/sober-plugin/src/audit.rs`
- `backend/crates/sober-plugin/src/registry.rs`

Each file should contain just a comment placeholder, e.g. `//! TODO`.

- [ ] **Step 5: Verify**

Run: `cargo build -p sober-plugin -q`

- [ ] **Step 6: Commit**

```bash
git add backend/Cargo.toml backend/crates/sober-plugin/
git commit -m "feat(plugin): scaffold sober-plugin crate with module structure"
```

---

## Task 11: Implement PluginError

**Files:**
- Modify: `backend/crates/sober-plugin/src/error.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let e = PluginError::NotFound("test-plugin".into());
        assert_eq!(e.to_string(), "plugin not found: test-plugin");

        let e = PluginError::ManifestInvalid("missing name".into());
        assert_eq!(e.to_string(), "manifest invalid: missing name");
    }

    #[test]
    fn into_app_error() {
        let e = PluginError::NotFound("x".into());
        let app: sober_core::error::AppError = e.into();
        assert!(matches!(app, sober_core::error::AppError::NotFound(_)));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sober-plugin -q`
Expected: compile error (PluginError not defined)

- [ ] **Step 3: Implement PluginError**

```rust
//! Plugin system error types.

use sober_core::error::AppError;

/// Errors that can occur in the plugin system.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// Plugin not found in registry.
    #[error("plugin not found: {0}")]
    NotFound(String),

    /// Audit pipeline rejected the plugin.
    #[error("audit rejected at {stage}: {reason}")]
    AuditRejected {
        /// Which audit stage failed.
        stage: String,
        /// Why it failed.
        reason: String,
    },

    /// Plugin tried to use an undeclared capability.
    #[error("capability denied: {0}")]
    CapabilityDenied(String),

    /// Plugin execution failed at runtime.
    #[error("plugin execution failed: {0}")]
    ExecutionFailed(String),

    /// WASM compilation failed.
    #[error("compilation failed: {0}")]
    CompilationFailed(String),

    /// Plugin manifest is invalid.
    #[error("manifest invalid: {0}")]
    ManifestInvalid(String),

    /// A plugin with this name already exists in the same scope.
    #[error("plugin already exists: {0}")]
    AlreadyExists(String),

    /// Internal error.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<PluginError> for AppError {
    fn from(e: PluginError) -> Self {
        match e {
            PluginError::NotFound(msg) => AppError::NotFound(msg),
            PluginError::ManifestInvalid(msg) => AppError::Validation(msg),
            PluginError::AlreadyExists(msg) => AppError::Conflict(msg),
            PluginError::CapabilityDenied(msg) => AppError::Forbidden(msg),
            PluginError::AuditRejected { stage, reason } => {
                AppError::Validation(format!("audit rejected at {stage}: {reason}"))
            }
            PluginError::ExecutionFailed(msg)
            | PluginError::CompilationFailed(msg) => AppError::Internal(msg.into()),
            PluginError::Internal(e) => AppError::Internal(e),
        }
    }
}
```

Note: Check the actual `AppError` variants before implementing. The `From`
impl should map to the correct existing variants (`NotFound`, `Validation`,
`Conflict`, `Forbidden`, `Internal`).

- [ ] **Step 4: Run tests**

Run: `cargo test -p sober-plugin -q`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-plugin/src/error.rs
git commit -m "feat(plugin): implement PluginError with AppError conversion"
```

---

## Task 12: Implement capability types

**Files:**
- Modify: `backend/crates/sober-plugin/src/capability.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_enabled_bool() {
        let c: Cap<NetworkCap> = Cap::Enabled(true);
        assert!(c.is_enabled());
    }

    #[test]
    fn cap_disabled_bool() {
        let c: Cap<NetworkCap> = Cap::Enabled(false);
        assert!(!c.is_enabled());
    }

    #[test]
    fn cap_default_is_disabled() {
        let c: Cap<NetworkCap> = Cap::default();
        assert!(!c.is_enabled());
    }

    #[test]
    fn cap_with_config() {
        let c: Cap<NetworkCap> = Cap::WithConfig(NetworkCap {
            domains: vec!["example.com".into()],
        });
        assert!(c.is_enabled());
    }

    #[test]
    fn capabilities_config_to_capabilities() {
        let mut config = CapabilitiesConfig::default();
        config.network = Cap::WithConfig(NetworkCap {
            domains: vec!["example.com".into()],
        });
        config.key_value = Cap::Enabled(true);

        let caps = config.to_capabilities();
        assert_eq!(caps.len(), 2);
    }

    #[test]
    fn empty_config_produces_no_capabilities() {
        let config = CapabilitiesConfig::default();
        let caps = config.to_capabilities();
        assert!(caps.is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sober-plugin -q`

- [ ] **Step 3: Implement capability types**

```rust
//! Capability types for WASM plugin isolation.
//!
//! Each capability maps to a set of host functions wired into the plugin's
//! WASM instance. MCP and Skill plugins do not use the capability system.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A typed capability that a WASM plugin can declare.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum Capability {
    /// Read from vector memory (paginated).
    MemoryRead { scopes: Vec<String> },
    /// Write to vector memory.
    MemoryWrite { scopes: Vec<String> },
    /// HTTP requests to allowed domains.
    Network { domains: Vec<String> },
    /// Read/write workspace files at allowed paths.
    Filesystem { paths: Vec<PathBuf> },
    /// Call LLM for reasoning.
    LlmCall,
    /// Invoke other registered tools by name.
    ToolCall { tools: Vec<String> },
    /// Read conversation messages (paginated).
    ConversationRead,
    /// Emit metrics (counters, gauges, histograms).
    Metrics,
    /// Read decrypted secrets by name.
    SecretRead,
    /// Plugin-local persistent key-value store.
    KeyValue,
    /// Schedule future self-invocations.
    Schedule,
}

/// Accepts both `true` and `{ field = ... }` in TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Cap<T> {
    /// Simple boolean: `true` = enabled, `false` = disabled.
    Enabled(bool),
    /// Enabled with configuration.
    WithConfig(T),
}

impl<T> Cap<T> {
    /// Returns `true` if this capability is enabled (either bool or config).
    pub fn is_enabled(&self) -> bool {
        match self {
            Cap::Enabled(b) => *b,
            Cap::WithConfig(_) => true,
        }
    }
}

impl<T> Default for Cap<T> {
    fn default() -> Self {
        Cap::Enabled(false)
    }
}

/// Network capability config.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkCap {
    /// Allowed domains for HTTP requests.
    pub domains: Vec<String>,
}

/// Memory capability config.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryCap {
    /// Allowed memory scopes (e.g., "user", "session").
    pub scopes: Vec<String>,
}

/// Filesystem capability config.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FilesystemCap {
    /// Allowed filesystem paths.
    pub paths: Vec<String>,
}

/// ToolCall capability config.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCallCap {
    /// Names of tools this plugin may invoke.
    pub tools: Vec<String>,
}

/// Flat TOML representation of capabilities.
///
/// Each field accepts `true` or an inline table with typed config.
/// See [`Cap`] for the deserialization logic.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitiesConfig {
    /// Read from vector memory.
    #[serde(default)]
    pub memory_read: Cap<MemoryCap>,
    /// Write to vector memory.
    #[serde(default)]
    pub memory_write: Cap<MemoryCap>,
    /// HTTP requests to allowed domains.
    #[serde(default)]
    pub network: Cap<NetworkCap>,
    /// Read/write workspace files.
    #[serde(default)]
    pub filesystem: Cap<FilesystemCap>,
    /// Invoke other tools by name.
    #[serde(default)]
    pub tool_call: Cap<ToolCallCap>,
    /// Call LLM for reasoning.
    #[serde(default)]
    pub llm_call: Cap<()>,
    /// Read conversation messages.
    #[serde(default)]
    pub conversation_read: Cap<()>,
    /// Emit metrics.
    #[serde(default)]
    pub metrics: Cap<()>,
    /// Read decrypted secrets.
    #[serde(default)]
    pub secret_read: Cap<()>,
    /// Plugin-local key-value store.
    #[serde(default)]
    pub key_value: Cap<()>,
    /// Schedule future invocations.
    #[serde(default)]
    pub schedule: Cap<()>,
}

impl CapabilitiesConfig {
    /// Converts the flat config to a list of typed capabilities.
    ///
    /// Only enabled capabilities are included.
    pub fn to_capabilities(&self) -> Vec<Capability> {
        let mut caps = Vec::new();

        if let Cap::WithConfig(ref c) = self.memory_read {
            caps.push(Capability::MemoryRead { scopes: c.scopes.clone() });
        } else if self.memory_read.is_enabled() {
            caps.push(Capability::MemoryRead { scopes: vec![] });
        }

        if let Cap::WithConfig(ref c) = self.memory_write {
            caps.push(Capability::MemoryWrite { scopes: c.scopes.clone() });
        } else if self.memory_write.is_enabled() {
            caps.push(Capability::MemoryWrite { scopes: vec![] });
        }

        if let Cap::WithConfig(ref c) = self.network {
            caps.push(Capability::Network { domains: c.domains.clone() });
        } else if self.network.is_enabled() {
            caps.push(Capability::Network { domains: vec![] });
        }

        if let Cap::WithConfig(ref c) = self.filesystem {
            caps.push(Capability::Filesystem {
                paths: c.paths.iter().map(PathBuf::from).collect(),
            });
        } else if self.filesystem.is_enabled() {
            caps.push(Capability::Filesystem { paths: vec![] });
        }

        if let Cap::WithConfig(ref c) = self.tool_call {
            caps.push(Capability::ToolCall { tools: c.tools.clone() });
        } else if self.tool_call.is_enabled() {
            caps.push(Capability::ToolCall { tools: vec![] });
        }

        if self.llm_call.is_enabled() { caps.push(Capability::LlmCall); }
        if self.conversation_read.is_enabled() { caps.push(Capability::ConversationRead); }
        if self.metrics.is_enabled() { caps.push(Capability::Metrics); }
        if self.secret_read.is_enabled() { caps.push(Capability::SecretRead); }
        if self.key_value.is_enabled() { caps.push(Capability::KeyValue); }
        if self.schedule.is_enabled() { caps.push(Capability::Schedule); }

        caps
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p sober-plugin -q`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-plugin/src/capability.rs
git commit -m "feat(plugin): implement Capability enum, Cap<T>, and CapabilitiesConfig"
```

---

## Task 13: Implement manifest parsing

**Files:**
- Modify: `backend/crates/sober-plugin/src/manifest.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_TOML: &str = r#"
[plugin]
name = "test-plugin"
version = "0.1.0"
description = "A test plugin"

[capabilities]
network = { domains = ["example.com"] }
key_value = true

[[tools]]
name = "do_thing"
description = "Does a thing"

[[metrics]]
name = "things_done"
kind = "counter"
description = "How many things done"
"#;

    #[test]
    fn parse_valid_manifest() {
        let manifest = PluginManifest::from_toml(SAMPLE_TOML).unwrap();
        assert_eq!(manifest.plugin.name, "test-plugin");
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.metrics.len(), 1);

        let caps = manifest.capabilities.to_capabilities();
        assert_eq!(caps.len(), 2);
    }

    #[test]
    fn parse_minimal_manifest() {
        let toml = r#"
[plugin]
name = "minimal"
version = "0.1.0"

[[tools]]
name = "my_tool"
description = "A tool"
"#;
        let manifest = PluginManifest::from_toml(toml).unwrap();
        assert_eq!(manifest.plugin.name, "minimal");
        assert!(manifest.capabilities.to_capabilities().is_empty());
    }

    #[test]
    fn reject_missing_name() {
        let toml = r#"
[plugin]
version = "0.1.0"

[[tools]]
name = "t"
description = "d"
"#;
        assert!(PluginManifest::from_toml(toml).is_err());
    }

    #[test]
    fn reject_empty_tools() {
        let toml = r#"
[plugin]
name = "no-tools"
version = "0.1.0"
"#;
        let result = PluginManifest::from_toml(toml);
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sober-plugin -q`

- [ ] **Step 3: Implement manifest types**

```rust
//! WASM plugin manifest (`plugin.toml`) parsing and validation.

use serde::{Deserialize, Serialize};

use crate::capability::CapabilitiesConfig;
use crate::error::PluginError;

/// Parsed `plugin.toml` manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Plugin metadata.
    pub plugin: PluginMeta,
    /// Declared capabilities.
    #[serde(default)]
    pub capabilities: CapabilitiesConfig,
    /// Tools exported by the plugin.
    #[serde(default)]
    pub tools: Vec<ToolEntry>,
    /// Metric declarations (required when metrics capability is enabled).
    #[serde(default)]
    pub metrics: Vec<MetricDeclaration>,
}

/// Plugin metadata from the `[plugin]` section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    /// Plugin name (unique within scope).
    pub name: String,
    /// Semver version string.
    pub version: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,
}

/// A tool exported by the plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEntry {
    /// Tool name (must match an exported function).
    pub name: String,
    /// Human-readable description.
    pub description: String,
}

/// A metric declared by the plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDeclaration {
    /// Metric name.
    pub name: String,
    /// Metric kind: "counter", "gauge", or "histogram".
    pub kind: String,
    /// Human-readable description.
    pub description: String,
}

impl PluginManifest {
    /// Parses and validates a `plugin.toml` string.
    pub fn from_toml(content: &str) -> Result<Self, PluginError> {
        let manifest: Self = toml::from_str(content)
            .map_err(|e| PluginError::ManifestInvalid(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validates manifest invariants.
    fn validate(&self) -> Result<(), PluginError> {
        if self.plugin.name.is_empty() {
            return Err(PluginError::ManifestInvalid(
                "plugin name must not be empty".into(),
            ));
        }
        if self.tools.is_empty() {
            return Err(PluginError::ManifestInvalid(
                "plugin must export at least one tool".into(),
            ));
        }
        // If metrics capability is enabled, metrics declarations must exist.
        if self.capabilities.metrics.is_enabled() && self.metrics.is_empty() {
            return Err(PluginError::ManifestInvalid(
                "metrics capability enabled but no [[metrics]] declared".into(),
            ));
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p sober-plugin -q`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-plugin/src/manifest.rs
git commit -m "feat(plugin): implement PluginManifest parsing and validation"
```

---

## Task 14: Implement audit pipeline

**Files:**
- Modify: `backend/crates/sober-plugin/src/audit.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sober_core::types::enums::{PluginKind, PluginOrigin};

    #[test]
    fn validate_mcp_valid_config() {
        let request = AuditRequest {
            name: "test-mcp".into(),
            kind: PluginKind::Mcp,
            origin: PluginOrigin::User,
            config: serde_json::json!({
                "command": "node",
                "args": ["server.js"],
                "env": {}
            }),
            manifest: None,
            wasm_bytes: None,
        };
        let report = AuditPipeline::audit(&request);
        assert!(matches!(report.verdict, AuditVerdict::Approved));
    }

    #[test]
    fn validate_mcp_missing_command() {
        let request = AuditRequest {
            name: "bad-mcp".into(),
            kind: PluginKind::Mcp,
            origin: PluginOrigin::User,
            config: serde_json::json!({"args": []}),
            manifest: None,
            wasm_bytes: None,
        };
        let report = AuditPipeline::audit(&request);
        assert!(matches!(report.verdict, AuditVerdict::Rejected { .. }));
    }

    #[test]
    fn validate_skill_valid() {
        let request = AuditRequest {
            name: "test-skill".into(),
            kind: PluginKind::Skill,
            origin: PluginOrigin::User,
            config: serde_json::json!({"path": "/home/user/.sober/skills/test/SKILL.md"}),
            manifest: None,
            wasm_bytes: None,
        };
        let report = AuditPipeline::audit(&request);
        assert!(matches!(report.verdict, AuditVerdict::Approved));
    }

    #[test]
    fn validate_wasm_with_manifest() {
        let manifest = crate::manifest::PluginManifest::from_toml(r#"
[plugin]
name = "test"
version = "0.1.0"

[[tools]]
name = "t"
description = "d"
"#).unwrap();

        let request = AuditRequest {
            name: "test-wasm".into(),
            kind: PluginKind::Wasm,
            origin: PluginOrigin::Agent,
            config: serde_json::json!({}),
            manifest: Some(manifest),
            wasm_bytes: None,
        };
        let report = AuditPipeline::audit(&request);
        // Validate stage passes (manifest is valid).
        // Sandbox/capability/test stages are skipped (no wasm_bytes in Plan A).
        assert!(matches!(report.verdict, AuditVerdict::Approved));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sober-plugin -q`

- [ ] **Step 3: Implement audit pipeline**

```rust
//! Type-aware plugin audit pipeline.
//!
//! All plugins go through audit. Stages differ by kind:
//! - MCP: validate config shape only
//! - Skill: validate path is present
//! - WASM: validate manifest (sandbox/capability/test stages added in Plan B)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sober_core::types::enums::{PluginKind, PluginOrigin};

use crate::manifest::PluginManifest;

/// Input to the audit pipeline.
pub struct AuditRequest {
    /// Plugin name.
    pub name: String,
    /// Plugin kind.
    pub kind: PluginKind,
    /// Where the plugin came from.
    pub origin: PluginOrigin,
    /// Plugin config (JSONB from the install request).
    pub config: serde_json::Value,
    /// Parsed manifest (WASM only).
    pub manifest: Option<PluginManifest>,
    /// Compiled WASM bytes (WASM only, added in Plan B).
    pub wasm_bytes: Option<Vec<u8>>,
}

/// Result of a single audit stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    /// Stage name.
    pub name: String,
    /// Whether the stage passed.
    pub passed: bool,
    /// Optional details on failure.
    pub details: Option<String>,
}

/// Overall audit verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditVerdict {
    /// All stages passed.
    Approved,
    /// A stage failed.
    Rejected {
        /// Which stage failed.
        stage: String,
        /// Why it failed.
        reason: String,
    },
}

/// Complete audit report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    /// Plugin name.
    pub plugin_name: String,
    /// Plugin kind.
    pub plugin_kind: PluginKind,
    /// Origin.
    pub origin: PluginOrigin,
    /// Results of each stage.
    pub stages: Vec<StageResult>,
    /// Final verdict.
    pub verdict: AuditVerdict,
    /// When the audit ran.
    pub timestamp: DateTime<Utc>,
}

/// The audit pipeline.
///
/// Runs type-specific validation stages and produces an [`AuditReport`].
pub struct AuditPipeline;

impl AuditPipeline {
    /// Run the audit pipeline for a plugin install request.
    pub fn audit(request: &AuditRequest) -> AuditReport {
        let mut stages = Vec::new();

        // Stage 1: Validate (all kinds)
        let validate = match request.kind {
            PluginKind::Mcp => Self::validate_mcp(&request.config),
            PluginKind::Skill => Self::validate_skill(&request.config),
            PluginKind::Wasm => Self::validate_wasm(&request.manifest),
        };
        stages.push(validate);

        // Determine verdict from stages.
        let verdict = if let Some(failed) = stages.iter().find(|s| !s.passed) {
            AuditVerdict::Rejected {
                stage: failed.name.clone(),
                reason: failed.details.clone().unwrap_or_default(),
            }
        } else {
            AuditVerdict::Approved
        };

        AuditReport {
            plugin_name: request.name.clone(),
            plugin_kind: request.kind,
            origin: request.origin,
            stages,
            verdict,
            timestamp: Utc::now(),
        }
    }

    fn validate_mcp(config: &serde_json::Value) -> StageResult {
        let passed = config.get("command").and_then(|v| v.as_str()).is_some();
        StageResult {
            name: "validate".into(),
            passed,
            details: if passed {
                None
            } else {
                Some("MCP config must include a 'command' string".into())
            },
        }
    }

    fn validate_skill(config: &serde_json::Value) -> StageResult {
        let passed = config.get("path").and_then(|v| v.as_str()).is_some();
        StageResult {
            name: "validate".into(),
            passed,
            details: if passed {
                None
            } else {
                Some("Skill config must include a 'path' string".into())
            },
        }
    }

    fn validate_wasm(manifest: &Option<PluginManifest>) -> StageResult {
        match manifest {
            Some(_) => StageResult {
                name: "validate".into(),
                passed: true,
                details: None,
            },
            None => StageResult {
                name: "validate".into(),
                passed: false,
                details: Some("WASM plugin requires a valid manifest".into()),
            },
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p sober-plugin -q`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-plugin/src/audit.rs
git commit -m "feat(plugin): implement AuditPipeline with validate stage per kind"
```

---

## Task 15: Implement PluginRegistry

**Files:**
- Modify: `backend/crates/sober-plugin/src/registry.rs`

- [ ] **Step 1: Implement PluginRegistry**

The registry orchestrates install (audit + DB write) and CRUD operations.
It is generic over `PluginRepo` so it can be tested with any backend.

```rust
//! Plugin registry — the public API for managing plugins.

use sober_core::error::AppError;
use sober_core::types::enums::{PluginKind, PluginScope, PluginStatus};
use sober_core::types::ids::PluginId;
use sober_core::types::input::{CreatePlugin, CreatePluginAuditLog, PluginFilter};
use sober_core::types::domain::Plugin;
use sober_core::types::repo::PluginRepo;

use crate::audit::{AuditPipeline, AuditReport, AuditRequest, AuditVerdict};
use crate::error::PluginError;
use crate::manifest::PluginManifest;

/// Request to install a plugin.
pub struct InstallRequest {
    /// Plugin name.
    pub name: String,
    /// Plugin kind.
    pub kind: PluginKind,
    /// Version (WASM only).
    pub version: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// Origin.
    pub origin: sober_core::types::enums::PluginOrigin,
    /// Scope.
    pub scope: PluginScope,
    /// Owner user ID.
    pub owner_id: Option<sober_core::types::ids::UserId>,
    /// Workspace ID (for workspace-scoped plugins).
    pub workspace_id: Option<sober_core::types::ids::WorkspaceId>,
    /// Kind-specific config (JSONB).
    pub config: serde_json::Value,
    /// Who is installing this plugin.
    pub installed_by: Option<sober_core::types::ids::UserId>,
    /// Parsed manifest (WASM only).
    pub manifest: Option<PluginManifest>,
    /// Compiled WASM bytes (WASM only, Plan B).
    pub wasm_bytes: Option<Vec<u8>>,
}

/// The plugin registry.
pub struct PluginRegistry<P> {
    db: P,
}

impl<P: PluginRepo> PluginRegistry<P> {
    /// Creates a new registry backed by the given repo.
    pub fn new(db: P) -> Self {
        Self { db }
    }

    /// Installs a plugin: runs audit, stores record if approved.
    pub async fn install(&self, request: InstallRequest) -> Result<AuditReport, PluginError> {
        // Check for duplicates.
        let existing = self
            .db
            .get_by_name(&request.name, request.scope, request.owner_id, request.workspace_id)
            .await
            .map_err(|e| PluginError::Internal(e.into()))?;
        if existing.is_some() {
            return Err(PluginError::AlreadyExists(request.name.clone()));
        }

        // Run audit.
        let audit_request = AuditRequest {
            name: request.name.clone(),
            kind: request.kind,
            origin: request.origin,
            config: request.config.clone(),
            manifest: request.manifest.clone(),
            wasm_bytes: request.wasm_bytes.clone(),
        };
        let report = AuditPipeline::audit(&audit_request);

        // Store audit log.
        let audit_log_input = CreatePluginAuditLog {
            plugin_id: None, // set after plugin is created
            plugin_name: request.name.clone(),
            kind: request.kind,
            origin: request.origin,
            stages: serde_json::to_value(&report.stages)
                .unwrap_or_else(|_| serde_json::json!([])),
            verdict: match &report.verdict {
                AuditVerdict::Approved => "approved".into(),
                AuditVerdict::Rejected { .. } => "rejected".into(),
            },
            rejection_reason: match &report.verdict {
                AuditVerdict::Rejected { reason, .. } => Some(reason.clone()),
                _ => None,
            },
            audited_by: request.installed_by,
        };

        match &report.verdict {
            AuditVerdict::Approved => {
                // Create plugin record.
                let plugin = self
                    .db
                    .create(CreatePlugin {
                        name: request.name,
                        kind: request.kind,
                        version: request.version,
                        description: request.description,
                        origin: request.origin,
                        scope: request.scope,
                        owner_id: request.owner_id,
                        workspace_id: request.workspace_id,
                        status: PluginStatus::Enabled,
                        config: request.config,
                        installed_by: request.installed_by,
                    })
                    .await
                    .map_err(|e| PluginError::Internal(e.into()))?;

                // Store audit log with plugin_id.
                let mut log_input = audit_log_input;
                log_input.plugin_id = Some(plugin.id);
                let _ = self.db.create_audit_log(log_input).await;
            }
            AuditVerdict::Rejected { .. } => {
                // Store audit log without plugin record.
                let _ = self.db.create_audit_log(audit_log_input).await;
            }
        }

        Ok(report)
    }

    /// Uninstalls a plugin by ID.
    pub async fn uninstall(&self, id: PluginId) -> Result<(), PluginError> {
        self.db
            .delete(id)
            .await
            .map_err(|e| match e {
                AppError::NotFound(msg) => PluginError::NotFound(msg),
                other => PluginError::Internal(other.into()),
            })
    }

    /// Enables a disabled plugin.
    pub async fn enable(&self, id: PluginId) -> Result<(), PluginError> {
        self.db
            .update_status(id, PluginStatus::Enabled)
            .await
            .map_err(|e| match e {
                AppError::NotFound(msg) => PluginError::NotFound(msg),
                other => PluginError::Internal(other.into()),
            })
    }

    /// Disables an enabled plugin.
    pub async fn disable(&self, id: PluginId) -> Result<(), PluginError> {
        self.db
            .update_status(id, PluginStatus::Disabled)
            .await
            .map_err(|e| match e {
                AppError::NotFound(msg) => PluginError::NotFound(msg),
                other => PluginError::Internal(other.into()),
            })
    }

    /// Lists plugins with optional filters.
    pub async fn list(&self, filter: PluginFilter) -> Result<Vec<Plugin>, PluginError> {
        self.db
            .list(filter)
            .await
            .map_err(|e| PluginError::Internal(e.into()))
    }

    /// Gets a single plugin by ID.
    pub async fn get(&self, id: PluginId) -> Result<Plugin, PluginError> {
        self.db
            .get_by_id(id)
            .await
            .map_err(|e| match e {
                AppError::NotFound(msg) => PluginError::NotFound(msg),
                other => PluginError::Internal(other.into()),
            })
    }
}
```

- [ ] **Step 2: Verify**

Run: `cargo build -p sober-plugin -q`

- [ ] **Step 3: Verify all tests pass**

Run: `cargo test -p sober-plugin -q`
Expected: PASS

- [ ] **Step 4: Run clippy**

Run: `cargo clippy -p sober-plugin -q -- -D warnings`

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-plugin/src/registry.rs
git commit -m "feat(plugin): implement PluginRegistry with install, CRUD, and audit"
```

---

## Task 16: Final verification

- [ ] **Step 1: Build entire workspace**

Run: `cargo build -q`

- [ ] **Step 2: Run all sober-plugin tests**

Run: `cargo test -p sober-plugin -q`

- [ ] **Step 3: Run clippy on sober-plugin**

Run: `cargo clippy -p sober-plugin -q -- -D warnings`

- [ ] **Step 4: Run sober-core and sober-db tests**

Run: `cargo test -p sober-core -p sober-db -q`

- [ ] **Step 5: Verify no existing tests are broken**

Run: `cargo test --workspace -q`

---

## Acceptance Criteria

- [ ] `PluginId` newtype exists in sober-core
- [ ] `PluginKind`, `PluginOrigin`, `PluginScope`, `PluginStatus` enums with sqlx + serde derives
- [ ] `Plugin` and `PluginAuditLog` domain types
- [ ] `PluginRepo` trait with CRUD, audit logs, and KV data methods
- [ ] SQL migration creates `plugins`, `plugin_audit_logs`, `plugin_kv_data` tables
- [ ] `PgPluginRepo` implements all `PluginRepo` methods
- [ ] `PluginError` with `From<PluginError> for AppError`
- [ ] `Capability` enum with all 11 variants
- [ ] `Cap<T>` untagged enum for TOML bool/table deserialization
- [ ] `CapabilitiesConfig` with `to_capabilities()` conversion
- [ ] `PluginManifest::from_toml()` parses and validates `plugin.toml`
- [ ] `AuditPipeline::audit()` runs validate stage per kind (MCP, Skill, WASM)
- [ ] `PluginRegistry` orchestrates install (audit + DB) and CRUD
- [ ] All public items have doc comments
- [ ] No `.unwrap()` in library code
- [ ] `cargo clippy -p sober-plugin -q -- -D warnings` reports zero warnings
- [ ] `cargo test -p sober-plugin -q` passes
- [ ] `cargo test --workspace -q` passes (no regressions)
