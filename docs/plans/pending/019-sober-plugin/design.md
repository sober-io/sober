# 019 --- Unified Plugin System

**Date:** 2026-03-20 (revised)
**Status:** Pending
**Crates:** `sober-plugin`, `sober-plugin-gen`

---

## Overview

A unified plugin system that brings all agent extensibility under one registry,
lifecycle, and audit pipeline. Three plugin kinds --- MCP (external processes),
Skills (markdown prompt injection), and WASM (compiled in-process modules) ---
share a single `PluginManager` with type-aware behavior.

This is a revision of the original 019 plan. The original designed WASM plugins
as a standalone system. Since then, `sober-mcp` and `sober-skill` have been
fully implemented. This revision wraps both under `sober-plugin` and adds WASM
support, creating one coordinated system.

**Key changes from original design:**

- `sober-plugin` wraps `sober-mcp` and `sober-skill` (does not absorb them)
- Unified `plugins` DB table replaces `mcp_servers`; skills are registered on
  filesystem discovery
- Type-aware audit pipeline with kind-specific stages
- Self-evolution generates both Skills and WASM plugins (not WASM-only)
- MCP servers remain human-managed (no agent self-evolution for MCP)
- Expanded capability set with phased implementation

**Priority:** Post-v1. Nothing on the v1 critical path depends on this.

---

## 1. Core Model

A **plugin** is any extension to the agent's capabilities.

### Plugin kinds

| Kind | Execution | Isolation | Source of Truth |
|------|-----------|-----------|-----------------|
| `Mcp` | External process, stdio | bwrap (process sandbox) | PostgreSQL |
| `Skill` | Prompt injection (markdown) | None needed (read-only text) | Filesystem (DB tracks lifecycle state) |
| `Wasm` | In-process, Extism | WASM sandbox (capability-based) | PostgreSQL + filesystem |

### Enums

```rust
// All enums follow the project pattern: Debug, Clone, Copy, PartialEq, Eq,
// Hash, Serialize, Deserialize + feature-gated sqlx::Type.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(feature = "postgres", sqlx(type_name = "plugin_kind", rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum PluginKind { Mcp, Skill, Wasm }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(feature = "postgres", sqlx(type_name = "plugin_origin", rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum PluginOrigin { System, Agent, User }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(feature = "postgres", sqlx(type_name = "plugin_scope", rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum PluginScope { System, User, Workspace }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(feature = "postgres", sqlx(type_name = "plugin_status", rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum PluginStatus { Enabled, Disabled, Failed }
```

`PluginId` is generated via `define_id!(PluginId)` in `sober-core/src/types/ids.rs`.

### Lifecycle

```
DISCOVER --> AUDIT --> INSTALL
                         |
                   +-----+-----+
                   |           |
                Enabled     Failed
                   |
                Disabled
```

- Audit passes -> `Enabled` (all origins, all scopes)
- Audit fails -> `Failed`
- User can toggle `Enabled` <-> `Disabled`

No approval flow. The audit pipeline is the trust gate. If all stages pass
(validate, sandbox, capability, test), the plugin is enabled immediately.
This keeps the self-evolution loop low-friction. When behavioral analysis
(currently a stub) is implemented, it will provide stronger automated
protection than manual approval ever could.

All plugin kinds share the same lifecycle. Audit stages differ by kind.

---

## 2. Crate Architecture

```
sober-plugin (unified registry, lifecycle, audit, WASM host)
  +-- depends on: sober-mcp     (delegates MCP execution)
  +-- depends on: sober-skill   (delegates skill loading/activation)
  +-- depends on: sober-sandbox (WASM pre-install test execution)
  +-- depends on: sober-core    (Tool trait, AppError, repo traits)
  +-- new code:   Extism host, capability system, manifest, audit pipeline

sober-plugin-gen (separate crate -- generation factory)
  +-- depends on: sober-plugin  (compile + test via Extism host)
  +-- depends on: sober-llm    (LLM-powered code generation)
  +-- generates:  Skills (markdown) and WASM plugins (Rust source -> compiled)
```

### Changes to existing crates

| Crate | Change |
|-------|--------|
| `sober-core` | Add `PluginRepo` trait, `PluginId`, plugin domain types |
| `sober-db` | Add `PgPluginRepo`. Migration: create `plugins` table, migrate `mcp_servers` data |
| `sober-agent` | `ToolBootstrap` uses `PluginManager` instead of direct `McpPool` + `SkillLoader` |
| `sober-api` | Unified `/api/v1/plugins` routes replace separate MCP and Skill routes |
| `sober-mcp` | No changes (sober-plugin wraps it) |
| `sober-skill` | No changes (sober-plugin wraps it) |

### PluginManager

Central coordinating type. `McpPool` is per-user (each user has their own
MCP servers), so the manager creates/caches pools on demand:

```rust
pub struct PluginManager<P: PluginRepo> {
    db: P,
    mcp_pools: RwLock<HashMap<UserId, McpPool>>,  // per-user MCP pools
    skill_loader: SkillLoader,
    audit: AuditPipeline,
    wasm_hosts: RwLock<HashMap<PluginId, PluginHost>>,  // loaded on demand
}
```

---

## 3. Database Schema

### plugins table

Replaces `mcp_servers`. Tracks all three plugin kinds.

```sql
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
```

**`config` JSONB by kind:**

- MCP: `{ "command": "...", "args": [...], "env": {...} }`
- Skill: `{ "path": "/absolute/path/to/SKILL.md" }`
- WASM: `{ "wasm_hash": "...", "capabilities": [...] }`

### plugin_audit_logs table

```sql
CREATE TABLE plugin_audit_logs (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plugin_id        UUID REFERENCES plugins(id),
    plugin_name      TEXT NOT NULL,
    kind             plugin_kind NOT NULL,
    origin           plugin_origin NOT NULL,
    stages           JSONB NOT NULL,
    verdict          TEXT NOT NULL,
    rejection_reason TEXT,
    audited_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    audited_by       UUID REFERENCES users(id)
);
```

### Migration strategy

1. Create `plugins` table and `plugin_audit_logs` table
2. Migrate existing `mcp_servers` rows into `plugins`:
   - `kind` = `'mcp'`
   - `scope` = `'user'`
   - `owner_id` = `mcp_servers.user_id`
   - `status` = `CASE WHEN enabled THEN 'enabled' ELSE 'disabled' END`
   - `config` = `jsonb_build_object('command', command, 'args', args, 'env', env)`
   - `installed_by` = `mcp_servers.user_id`
   - `installed_at` = `mcp_servers.created_at`
   - `updated_at` = `mcp_servers.updated_at`
3. Drop `mcp_servers` table
4. Remove `McpServerRepo` trait from `sober-core` and `PgMcpServerRepo` from
   `sober-db`. `McpServerId` is also removed. `sober-mcp` does not use
   `McpServerId` internally (it uses `McpServerRunConfig` for connections).
5. Skills are registered on discovery by `PluginManager` (no data migration)

### plugin_kv_data table

Persistent key-value store for WASM plugins with the `KeyValue` capability.
One row per plugin — all KV data stored as a single JSONB blob.

```sql
CREATE TABLE plugin_kv_data (
    plugin_id  UUID PRIMARY KEY REFERENCES plugins(id) ON DELETE CASCADE,
    data       JSONB NOT NULL DEFAULT '{}',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

Host functions operate on keys within the `data` blob:

- `host_kv_get(key) -> Option<JsonValue>` — read `data[key]`
- `host_kv_set(key, value) -> ()` — set `data[key] = value`
- `host_kv_delete(key) -> ()` — remove `data[key]`
- `host_kv_list(prefix) -> Vec<String>` — list keys matching a prefix

The host function injects the calling plugin's `plugin_id` automatically.
Plugins cannot access another plugin's data.

### plugin_audit_logs FK behavior

`plugin_id` uses `ON DELETE SET NULL` so audit logs are preserved when plugins
are uninstalled:

```sql
plugin_id UUID REFERENCES plugins(id) ON DELETE SET NULL,
```

---

## 4. Capability System (WASM only)

Capabilities control which host functions are wired into a WASM plugin's
Extism instance. No capability declared = no host function available = plugin
physically cannot perform the action.

MCP and Skill plugins do not use the capability system. MCP has its own
sandbox policy (bwrap). Skills are read-only prompt text.

### Capability enum

```rust
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
```

### Host functions

**Always available (no capability gate):**

| Function | Purpose |
|----------|---------|
| `host_log(level, message, fields)` | Structured logging into tracing |

**Capability-gated:**

| Capability | Host Function | Signature |
|-----------|---------------|-----------|
| `Network` | `host_http_request` | `(method, url, headers, body) -> response` |
| `ToolCall` | `host_call_tool` | `(tool_name, input_json) -> output` |
| `SecretRead` | `host_read_secret` | `(name) -> value` |
| `Metrics` | `host_emit_metric` | `(name, kind, value, labels) -> ()` |
| `KeyValue` | `host_kv_get` / `host_kv_set` | `(key) -> value` / `(key, value) -> ()` |
| `MemoryRead` | `host_memory_query` | `(scope, query, cursor, limit) -> Page<MemoryChunk>` |
| `MemoryWrite` | `host_memory_write` | `(scope, key, value) -> ()` |
| `ConversationRead` | `host_conversation_read` | `(conversation_id, cursor, limit) -> Page<Message>` |
| `Schedule` | `host_schedule` | `(cron_or_interval, input_json) -> job_id` |
| `Filesystem` | `host_fs_read` / `host_fs_write` | `(path) -> bytes` / `(path, bytes) -> ()` |
| `LlmCall` | `host_llm_complete` | `(prompt, max_tokens) -> text` |

### Paginated responses

```rust
struct Page<T> {
    items: Vec<T>,
    next_cursor: Option<String>,
    has_more: bool,
}
```

### Metrics declaration in manifest

Plugins with the `Metrics` capability must declare their metrics:

```toml
[capabilities]
metrics = true

[[metrics]]
name = "plugin_items_processed"
kind = "counter"
description = "Number of items processed"
```

The host function validates emitted metrics match the declared set.

### Implementation phases

All capabilities are defined from day one (manifest format is stable).
Host function implementations are phased:

| Phase | Functional Capabilities |
|-------|------------------------|
| Phase 1 | `Network`, `ToolCall`, `SecretRead`, `Metrics`, `KeyValue` |
| Phase 2 | `MemoryRead`, `MemoryWrite`, `ConversationRead`, `Schedule` |
| Phase 3 | `Filesystem`, `LlmCall` |

Unimplemented host functions return
`PluginError::CapabilityDenied("capability not yet connected: ...")`.

### Subset check

Used for agent-origin auto-approval:

```rust
impl Capability {
    pub fn is_subset_of(requested: &[Capability], available: &[Capability]) -> bool;
}
```

---

## 5. Bidirectional Tool Interface

WASM plugins have a bidirectional relationship with the agent:

```
Agent                          WASM Plugin
  |                                |
  |-- invoke [[tools]] ---------->|  Agent calls plugin-exported tools
  |                                |  (plugin appears as Tool trait impl)
  |                                |
  |<-- host_call_tool ------------|  Plugin calls agent-registered tools
  |                                |  (via ToolCall capability)
```

**Agent → Plugin:** Each `[[tools]]` entry in the manifest becomes a
`PluginTool` implementing the `Tool` trait. The LLM sees these identically
to MCP or built-in tools and can call them.

**Plugin → Agent:** With the `ToolCall` capability, a plugin can invoke
any agent-registered tool via `host_call_tool(name, input)`. This enables
composition — a plugin can chain existing tools (web_search, recall, etc.)
into a higher-level operation.

This composition is key to self-evolution: the agent identifies a multi-step
pattern it keeps repeating, codifies it as a WASM plugin that chains existing
tools together, and exposes a single deterministic tool for that workflow.

### Extism Plugin Host

Extism (built on wasmtime) is the WASM runtime. Handles host/guest boundary
plumbing, memory allocation, serialization.

```rust
pub struct PluginHost {
    manifest: PluginManifest,
    plugin: extism::Plugin,
}

impl PluginHost {
    /// Load WASM bytes, wire host functions for declared capabilities.
    pub fn load(
        wasm_bytes: &[u8],
        manifest: &PluginManifest,
    ) -> Result<Self, PluginError>;

    /// Call a tool function exported by the plugin.
    pub fn call_tool(
        &mut self,
        tool_name: &str,
        input: serde_json::Value,
    ) -> Result<ToolOutput, PluginError>;
}
```

### PluginTool

Each `[[tools]]` entry in the manifest becomes a `PluginTool` implementing
the `Tool` trait from sober-core:

```rust
pub struct PluginTool {
    host: Arc<std::sync::Mutex<PluginHost>>,  // std::sync — WASM calls are blocking
    tool_entry: String,
    metadata: ToolMetadata,
}

impl Tool for PluginTool {
    fn metadata(&self) -> ToolMetadata { ... }
    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        // WASM execution is synchronous — use spawn_blocking to avoid
        // blocking the async runtime. Lock is held only during the
        // blocking call, never across .await points.
        ...
    }
}
```

---

## 6. Plugin Manifest (WASM only)

```toml
[plugin]
name = "date-formatter"
version = "0.1.0"
description = "Formats dates in Estonian locale"
origin = "agent"
scope = "workspace"

[capabilities]
# Inline tables — named fields describe what the capability allows
memory_read = { scopes = ["user", "session"] }
memory_write = { scopes = ["session"] }
network = { domains = ["api.example.com", "hooks.slack.com"] }
filesystem = { paths = ["/workspace/data", "/workspace/output"] }
tool_call = { tools = ["web_search", "recall"] }

# Boolean — capability is present with no additional config
llm_call = true
conversation_read = true
metrics = true
secret_read = true
key_value = true
schedule = true

[[tools]]
name = "format_date"
description = "Format a date in Estonian locale"

# Required when metrics = true. Undeclared metric names are rejected.
[[metrics]]
name = "dates_formatted"
kind = "counter"
description = "Number of dates formatted"
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    pub capabilities: CapabilitiesConfig,
    pub tools: Vec<ToolEntry>,
    #[serde(default)]
    pub metrics: Vec<MetricDeclaration>,
}
```

### CapabilitiesConfig

Each field accepts either `true` (enabled, no config) or an inline table
with typed fields. Deserialized via an untagged enum:

```rust
/// Accepts both `true` and `{ field = ... }` in TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Cap<T: Default> {
    Enabled(bool),
    WithConfig(T),
}

impl<T: Default> Default for Cap<T> {
    fn default() -> Self { Cap::Enabled(false) }
}

// Per-capability config structs
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkCap { pub domains: Vec<String> }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryCap { pub scopes: Vec<String> }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FilesystemCap { pub paths: Vec<String> }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCallCap { pub tools: Vec<String> }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitiesConfig {
    #[serde(default)]
    pub memory_read: Cap<MemoryCap>,
    #[serde(default)]
    pub memory_write: Cap<MemoryCap>,
    #[serde(default)]
    pub network: Cap<NetworkCap>,
    #[serde(default)]
    pub filesystem: Cap<FilesystemCap>,
    #[serde(default)]
    pub tool_call: Cap<ToolCallCap>,
    #[serde(default)]
    pub llm_call: Cap<()>,
    #[serde(default)]
    pub conversation_read: Cap<()>,
    #[serde(default)]
    pub metrics: Cap<()>,
    #[serde(default)]
    pub secret_read: Cap<()>,
    #[serde(default)]
    pub key_value: Cap<()>,
    #[serde(default)]
    pub schedule: Cap<()>,
}

impl CapabilitiesConfig {
    /// Convert to typed capability list (only enabled capabilities included).
    pub fn to_capabilities(&self) -> Vec<Capability>;
}
```

Input schemas are defined in code via derive macros (`schemars::JsonSchema`),
not as separate files:

```rust
#[derive(Deserialize, JsonSchema)]
pub struct FormatDateInput {
    pub date: String,
    pub locale: Option<String>,
}
```

### Example: Building and Deploying a WASM Plugin

**1. Scaffold:**

```bash
# In a workspace directory
soberctl plugin new date-formatter
```

Produces:

```
.sober/plugins/date-formatter/
  +-- plugin.toml
  +-- src/
      +-- lib.rs
```

**2. Write the plugin:**

`plugin.toml`:

```toml
[plugin]
name = "date-formatter"
version = "0.1.0"
description = "Formats dates in Estonian locale"

[capabilities]
key_value = true

[[tools]]
name = "format_date"
description = "Format a date in Estonian locale"
```

`src/lib.rs`:

```rust
use extism_pdk::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct FormatDateInput {
    date: String,
    locale: Option<String>,
}

#[derive(Serialize)]
struct FormatDateOutput {
    formatted: String,
}

#[plugin_fn]
pub fn format_date(input: Json<FormatDateInput>) -> FnResult<Json<FormatDateOutput>> {
    let locale = input.0.locale.as_deref().unwrap_or("et-EE");
    // ... formatting logic ...
    Ok(Json(FormatDateOutput {
        formatted: format!("{} ({})", input.0.date, locale),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_with_default_locale() {
        // ...
    }
}
```

**3. Build:**

```bash
cd .sober/plugins/date-formatter
cargo build --target wasm32-wasi --release
```

**4. Install:**

```bash
# From the workspace
soberctl plugin install .sober/plugins/date-formatter

# Output:
# Audit: validate ✓, sandbox ✓, capability ✓, test ✓
# Audit: validate ✓, sandbox ✓, capability ✓, test ✓
# Plugin "date-formatter" installed and enabled (workspace scope)
```

**5. Use:** The agent now sees `format_date` as a tool and can call it.

### Slash Command: /generate-plugin

The agent exposes a `/generate-plugin` slash command that triggers
`sober-plugin-gen` in the current workspace:

```
/generate-plugin "formats dates in Estonian locale with support for
different calendar systems"
```

Flow:

1. Agent receives the description
2. Delegates to `PluginGenerator::generate_wasm()` with
   `scope = Workspace` and `origin = Agent`
3. LLM generates source + tests in `.sober/plugins/<name>/`
4. Compiles to WASM, runs tests (self-correcting loop, max 3 retries)
5. Installs via `PluginManager.install()` — audit passes, enabled
6. Agent confirms: "Created plugin `date-formatter` with tool `format_date`.
   It's enabled in this workspace."

The same flow happens autonomously when the agent identifies a repeated
pattern (self-evolution), but `/generate-plugin` lets the user trigger
it explicitly.

---

## 7. Audit Pipeline

Type-aware progressive audit. All plugins go through audit, but stages
differ by kind.

### Stages by kind

| Stage | MCP | Skill | WASM |
|-------|-----|-------|------|
| **Validate** | Config well-formed (command, args, env shape) | Frontmatter valid | Manifest parses, capabilities well-formed |
| **Sandbox** | N/A | N/A | Extism loads with declared capabilities |
| **Capability** | N/A | N/A | Only declared host functions wired |
| **Test** | N/A | N/A | Embedded tests pass in Extism |
| **Static** | N/A | Content check (stub) | AST analysis (stub) |
| **Behavioral** | N/A | N/A | Runtime monitoring (stub) |

MCP servers only run the **Validate** stage at install time. No spawn or
handshake test --- MCP servers may depend on runtime conditions (network,
API keys, env vars) that are unavailable during configuration. Connectivity
is verified on first use by `McpPool`. If the server fails to start or
handshake, the error surfaces at execution time.

### Audit outcome

- All stages pass -> plugin is **enabled** immediately
- Any stage fails -> plugin status is **failed**, audit log records the rejection

No manual approval step. The audit pipeline is the trust gate. Origin and
scope are recorded for auditability but do not affect the outcome.

### Types

```rust
pub struct AuditPipeline;

impl AuditPipeline {
    pub async fn audit(&self, request: &AuditRequest) -> Result<AuditReport, PluginError>;
}

pub struct AuditReport {
    pub plugin_name: String,
    pub plugin_kind: PluginKind,
    pub origin: PluginOrigin,
    pub stages: Vec<StageResult>,
    pub verdict: AuditVerdict,
    pub timestamp: DateTime<Utc>,
}

pub struct StageResult {
    pub name: String,
    pub passed: bool,
    pub details: Option<String>,
}

pub enum AuditVerdict {
    Approved,
    Rejected { stage: String, reason: String },
}
```

---

## 8. Plugin Registry

```rust
pub struct PluginRegistry<P: PluginRepo> {
    db: P,
    audit: AuditPipeline,
}

impl<P: PluginRepo> PluginRegistry<P> {
    pub async fn install(&self, request: InstallRequest) -> Result<AuditReport, PluginError>;
    pub async fn uninstall(&self, plugin_id: PluginId) -> Result<(), PluginError>;
    pub async fn enable(&self, plugin_id: PluginId) -> Result<(), PluginError>;
    pub async fn disable(&self, plugin_id: PluginId) -> Result<(), PluginError>;
    pub async fn list(&self, filter: PluginFilter) -> Result<Vec<PluginInfo>, PluginError>;
}
```

### Install flow

1. Parse manifest/config for the plugin kind
2. Compile source to WASM (if WASM kind with source)
3. Run audit pipeline
4. If approved: store record in DB with `status = enabled`
5. If rejected: store audit log only (no plugin record), return rejection

### Plugin resolution

For WASM plugins, resolution follows scope precedence:
workspace > user > system (same-name plugin at higher scope shadows lower).

### WASM compilation

WASM plugins are compiled by shelling out to `cargo build`:

```
cargo build --target wasm32-wasi --release
```

Target is `wasm32-wasi` (WASI Preview 1), which is the target Extism supports.
Verify Extism compatibility with `wasm32-wasip2` during implementation; if
supported, prefer `wasip2` for WASI Preview 2 features.

### Plugin versioning

Upgrading a plugin to a new version replaces the existing record in-place
(same `id`, updated `version`, `config`, `updated_at`). The old WASM artifact
is overwritten. Audit logs provide a history trail. There is no side-by-side
versioning or automatic rollback in v1. If a new version fails audit, the
install is rejected and the old version remains active.

---

## 9. Self-Evolution

The agent can autonomously create Skills and WASM plugins.
`sober-plugin-gen` is the factory crate.

### Skill generation

- Agent identifies a repeated prompt pattern
- Generates SKILL.md with proper frontmatter and body
- Written to user/workspace skill directory
- Registered in `plugins` table via `PluginManager`
- Origin: `Agent`, audit passes -> enabled

### WASM generation

- Agent identifies a repeated deterministic operation
- Delegates to `sober-plugin-gen` with a `GenerateRequest`
- Generation pipeline:
  1. Build prompt from description + PDK trait + manifest format + capabilities
  2. LLM generates Rust source + tests
  3. Validate structural correctness
  4. Compile to WASM (`wasm32-wasi`)
  5. Load in Extism, run tests
  6. If fail: feed errors to LLM, retry (max 3)
  7. On success: return `GenerateResult`
- Source committed to workspace git repo (via sober-workspace)
- Installed through `PluginManager.install()` -> audit pipeline
- Origin: `Agent`, audit passes -> enabled

### sober-plugin-gen API

```rust
pub struct PluginGenerator {
    llm: Arc<dyn LlmEngine>,
}

impl PluginGenerator {
    /// LLM-powered WASM plugin generation with self-correcting loop.
    pub async fn generate_wasm(&self, request: GenerateRequest) -> Result<GenerateResult, GenError>;

    /// Generate a Skill (markdown) from description.
    pub async fn generate_skill(&self, request: SkillGenRequest) -> Result<SkillGenResult, GenError>;

    /// Scaffold a plugin template (no LLM).
    pub fn scaffold(&self, name: &str, kind: PluginKind) -> Result<PathBuf, GenError>;
}

pub struct GenerateRequest {
    pub description: String,
    pub suggested_scope: PluginScope,
    pub capabilities: Vec<Capability>,
    pub origin: PluginOrigin,
}

pub struct GenerateResult {
    pub source_path: PathBuf,
    pub wasm_bytes: Vec<u8>,
    pub manifest: PluginManifest,
    pub test_results: TestResults,
}
```

---

## 10. Agent Integration

### PluginManager in ToolBootstrap

`ToolBootstrap` switches from direct `McpPool` + `SkillLoader` to
`PluginManager`:

```rust
pub struct ToolBootstrap<R: AgentRepos> {
    pub plugin_manager: Arc<PluginManager<PgPluginRepo>>,
    // ... other tool configs unchanged
}
```

### tools_for_turn

```rust
impl<P: PluginRepo> PluginManager<P> {
    /// Returns all tools from enabled plugins for this turn.
    pub async fn tools_for_turn(
        &self,
        ctx: &PluginContext,
    ) -> Result<Vec<Arc<dyn Tool>>, PluginError> {
        let mut tools = Vec::new();
        tools.extend(self.mcp_tools(ctx).await?);
        if let Some(skill_tool) = self.skill_tool(ctx).await? {
            tools.push(skill_tool);
        }
        tools.extend(self.wasm_tools(ctx).await?);
        Ok(tools)
    }
}
```

### PluginContext

```rust
pub struct PluginContext {
    pub user_id: UserId,
    pub workspace_id: Option<WorkspaceId>,
    pub conversation_id: ConversationId,
    pub skill_activation_state: Option<Arc<Mutex<SkillActivationState>>>,
}
```

---

## 11. API Surface

Unified plugin routes:

```
GET    /api/v1/plugins                  # List (filter by kind, scope)
POST   /api/v1/plugins                  # Install (single plugin)
POST   /api/v1/plugins/import           # Import from config file (batch)
GET    /api/v1/plugins/:id              # Details
PATCH  /api/v1/plugins/:id              # Update (enable/disable, config)
DELETE /api/v1/plugins/:id              # Uninstall
GET    /api/v1/plugins/:id/audit        # Audit report
POST   /api/v1/plugins/reload           # Re-scan filesystem (skills)
```

Existing `/api/v1/mcp/servers` and `/api/v1/skills` routes are removed.
Frontend MCP and Skills pages become filtered views of `/settings/plugins`.

### MCP install methods

**Manual form entry:** User provides name, command, args, env via the UI.
Same as the current MCP settings page. `POST /api/v1/plugins` with
`kind = "mcp"` and config payload.

**Config file import:** User uploads or pastes a JSON config file compatible
with Claude Code's `.mcp.json` format:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
      "env": {}
    }
  }
}
```

`POST /api/v1/plugins/import` parses the file and creates one plugin record
per server entry (batch). Returns a list of created plugins with their
audit results.

**Package/URL install (deferred):** Future enhancement. User provides an npm
package name, git URL, or Docker image. System resolves the run command and
creates the plugin record. Not part of this plan.

---

## 12. Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("plugin not found: {0}")]
    NotFound(String),

    #[error("audit rejected: {stage} -- {reason}")]
    AuditRejected { stage: String, reason: String },

    #[error("capability denied: {0}")]
    CapabilityDenied(String),

    #[error("plugin execution failed: {0}")]
    ExecutionFailed(String),

    #[error("compilation failed: {0}")]
    CompilationFailed(String),

    #[error("manifest invalid: {0}")]
    ManifestInvalid(String),

    #[error("plugin already exists: {0}")]
    AlreadyExists(String),

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

// sober-plugin-gen
#[derive(Debug, thiserror::Error)]
pub enum GenError {
    #[error("generation failed after {attempts} attempts: {reason}")]
    GenerationFailed { attempts: u32, reason: String },

    #[error("compilation failed: {0}")]
    CompilationFailed(String),

    #[error("tests failed: {0}")]
    TestsFailed(String),

    #[error("scaffold failed: {0}")]
    ScaffoldFailed(String),

    #[error(transparent)]
    Llm(#[from] LlmError),

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}
```

`PluginError` maps to `AppError`:
- `NotFound` -> 404
- `AuditRejected` -> 422 Unprocessable Entity (audit failed)
- `CapabilityDenied` -> 403 Forbidden
- `ManifestInvalid` -> 400 Bad Request
- `AlreadyExists` -> 409 Conflict
- `ExecutionFailed` / `CompilationFailed` / `Internal` -> 500

---

## 13. Dependencies

### sober-plugin

| Crate | Purpose |
|-------|---------|
| `sober-core` | Tool trait, shared types, config, repo traits |
| `sober-mcp` | MCP client/pool delegation |
| `sober-skill` | Skill loader/catalog delegation |
| `sober-sandbox` | Pre-install test execution |
| `extism` | WASM plugin runtime |
| `serde` / `serde_json` / `toml` | Manifest parsing, config serialization |
| `schemars` | JSON Schema generation |
| `tracing` | Structured logging |
| `thiserror` | Error types |

### sober-plugin-gen

| Crate | Purpose |
|-------|---------|
| `sober-core` | Shared types |
| `sober-plugin` | Compile + test via Extism host |
| `sober-llm` | LLM-powered code generation |
| `tracing` | Structured logging |
| `thiserror` | Error types |

### Dependency flow

```
sober-agent -----> sober-plugin       (PluginManager for tool construction)
            -----> sober-plugin-gen   (trigger generation, post-v1)

sober-cli   ---x   (NO direct dep on sober-plugin or sober-plugin-gen)
                    soberctl uses gRPC to agent for plugin ops at runtime.
                    sober (offline) does not need plugin management.

sober-api   -----> sober-core         (plugin types only -- proxies via gRPC to agent)

sober-plugin -----> sober-mcp         (MCP execution delegation)
             -----> sober-skill       (skill loading delegation)
             -----> sober-sandbox     (pre-install test execution)
             -----> sober-core        (Tool trait, types)

sober-plugin-gen -> sober-plugin      (compile + test)
                 -> sober-llm         (LLM generation)
                 -> sober-core        (shared types)
```

**CLI plugin commands:** `soberctl plugin {list,install,enable,disable,remove}`
uses the agent's Unix admin socket (gRPC). This keeps `sober-cli` lightweight
and avoids pulling in Extism/MCP/Skill dependencies.

**API routing:** Plugin API routes in `sober-api` proxy to the agent process
via gRPC (consistent with how skill routes already work). The `PluginManager`
lives in the agent process, not the API process.

---

## 14. Impact on Existing Designs

| Area | Change |
|------|--------|
| `sober-core` | Add `PluginRepo` trait, `PluginId`, plugin domain types to `types/` |
| `sober-db` | Add `PgPluginRepo`. Migration creates `plugins` table, migrates `mcp_servers` |
| `sober-agent` | `ToolBootstrap` uses `PluginManager` instead of direct `McpPool` + `SkillLoader` |
| `sober-api` | Unified `/api/v1/plugins` routes. Remove `/api/v1/mcp/servers` and `/api/v1/skills` |
| `sober-mcp` | No changes (wrapped by sober-plugin) |
| `sober-skill` | No changes (wrapped by sober-plugin) |
| Frontend | Remove `/settings/mcp/` page. New unified `/settings/plugins` page with kind filter tabs (All / MCP / Skills / WASM). Plugin install flow with kind selection, config input, audit status, approval UI. |
| gRPC proto | Add plugin management RPCs if agent handles plugin ops |
