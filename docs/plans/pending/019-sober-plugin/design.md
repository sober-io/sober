# 019 --- sober-plugin & sober-plugin-gen

**Date:** 2026-03-07
**Status:** Pending (post-v1)
**Crates:** `sober-plugin`, `sober-plugin-gen`

---

## Overview

Two crates that provide a WASM-based plugin system for extending the agent's
capabilities at runtime.

- **`sober-plugin`** --- Runtime: plugin registry, Extism WASM host, capability
  enforcement, progressive audit pipeline, plugin loading and execution.
- **`sober-plugin-gen`** --- Generation: LLM-powered plugin source generation
  with test verification, template scaffolding, compilation to WASM, source
  management via git.

Plugins are distinct from MCP servers (`sober-mcp`). MCP handles external
processes communicating over stdio. Plugins are compiled WASM modules running
inside the Sober process via Extism, with typed host/guest interfaces and
capability-based isolation.

**Priority:** Post-v1. Nothing on the v1 critical path depends on plugins.
The agent uses the `Tool` trait (sober-core), MCP tools (sober-mcp), and
built-in tools without any plugin infrastructure.

---

## 1. Plugin Origins & Trust

Plugins enter the system from three sources with different trust levels:

| Origin | Description | Approval |
|--------|-------------|----------|
| System | Shipped with Sober, read-only | Pre-approved |
| Agent | Agent identifies a repeated pattern and proposes a plugin | Auto-approve if capabilities are a subset of agent's existing access; otherwise admin approval |
| User/External | User or admin provides a plugin (source or pre-compiled WASM) | Always requires explicit user approval after audit |

Both agent-generated and user-installed plugins go through the same audit
pipeline. The difference is the approval threshold.

### Self-evolution

The agent can autonomously propose plugins when it identifies predictable,
repeated patterns that would be more efficient as deterministic WASM code
than repeated LLM calls. The agent decides *what* and *why* (using LLM
reasoning), then delegates the mechanical work to `sober-plugin-gen`. The
agent proposes a storage scope (user or workspace) based on context; the
approval step confirms.

---

## 2. Storage & Resolution

### Three-layer plugin storage

Mirrors the SOUL.md resolution pattern:

```
system plugins (/usr/share/sober/plugins/) --- read-only, shipped
  -> user plugins (~/.sober/plugins/)
    -> workspace plugins (.sober/plugins/)
```

Workspace overrides user, user overrides system. Same-name plugin at a higher
layer shadows the lower one.

### Filesystem layout

```
<scope-root>/plugins/
  <plugin-name>/
    src/          # git-managed source (Rust or TypeScript)
    plugin.wasm   # compiled artifact (ephemeral, rebuilt from source)
```

System plugins omit the `src/` directory --- they ship as pre-compiled WASM.

### Storage backends

| Backend | What | Purpose |
|---------|------|---------|
| Git | Plugin source code | Version history, diffs, source of truth |
| PostgreSQL | Plugin metadata, audit logs | Operational state, queries |
| Filesystem | Compiled WASM artifacts | Runtime loading (ephemeral, rebuilt from source) |

---

## 3. Plugin Format

### Manifest (`plugin.toml`)

```toml
[plugin]
name = "date-formatter"
version = "0.1.0"
description = "Formats dates in Estonian locale"
origin = "agent"       # or "user", "system"
scope = "workspace"    # or "user", "system"

[capabilities]
memory_read = ["user"]
network = []
filesystem = []

[[tools]]
name = "format_date"
description = "Format a date in Estonian locale"
```

### Project structure

```
my-plugin/
+-- plugin.toml
+-- src/
    +-- lib.rs    # implementation + #[cfg(test)] mod tests
```

Input schemas are defined in code via derive macros (`schemars::JsonSchema`),
not as separate files. The derived JSON Schema populates
`ToolMetadata.input_schema` automatically.

```rust
#[derive(Deserialize, JsonSchema)]
pub struct FormatDateInput {
    pub date: String,
    pub locale: Option<String>,
}
```

---

## 4. Capability System

### Capability types

```rust
pub enum Capability {
    MemoryRead(Vec<ScopeKind>),
    MemoryWrite(Vec<ScopeKind>),
    Network(Vec<String>),          // allowed domains
    Filesystem(Vec<PathBuf>),      // allowed paths
    LlmCall,                       // can call LLM via host function
    ToolCall(Vec<String>),         // can invoke other tools by name
}
```

Each capability maps to a set of Extism host functions wired into the plugin's
WASM instance. No capability declared = no host function available = plugin
physically cannot perform that action.

### Host functions (wired per capability)

| Capability | Host function | Signature |
|-----------|---------------|-----------|
| `MemoryRead` | `host_memory_read` | `(scope, query) -> results` |
| `MemoryWrite` | `host_memory_write` | `(scope, key, value) -> ok` |
| `Network` | `host_http_request` | `(method, url, headers, body) -> response` |
| `LlmCall` | `host_llm_complete` | `(prompt, max_tokens) -> text` |
| `ToolCall` | `host_call_tool` | `(tool_name, input_json) -> output` |

WASM modules have no I/O by default --- the WASM sandbox guarantees this.
Network restrictions are additionally enforced by Extism's `allowed_hosts`
manifest field and our host function implementation (domain filtering + audit
logging).

---

## 5. Extism Runtime

Extism (built on wasmtime) is the plugin runtime. It handles WASM host/guest
boundary plumbing, memory allocation, serialization, and provides multi-language
PDK support (Rust, TypeScript, Go, Python, etc.).

### Plugin host

```rust
pub struct PluginHost {
    manifest: PluginManifest,
    plugin: extism::Plugin,
}

impl PluginHost {
    pub fn load(
        wasm_bytes: &[u8],
        manifest: &PluginManifest,
    ) -> Result<Self, PluginError>;
}
```

### Plugin as Tool

Each `[[tools]]` entry in `plugin.toml` becomes a `Tool` implementation from
sober-core. The agent sees plugin tools identically to MCP tools or built-in
tools.

```rust
pub struct PluginTool {
    host: Arc<PluginHost>,
    tool_entry: String,       // exported function name
    metadata: ToolMetadata,
}

impl Tool for PluginTool { ... }
```

---

## 6. Audit Pipeline

Progressive stages. Capability enforcement is essential from day one; static
and behavioral analysis start as pass-through stubs.

### Stages

```
1. VALIDATE    -> Parse plugin.toml, verify structure, check capability declarations
2. COMPILE     -> Build source to WASM (skip if pre-compiled)
3. CAPABILITY  -> Wire only declared host functions, load WASM in Extism
4. TEST        -> Run embedded #[cfg(test)] tests in sandboxed Extism instance
5. STATIC      -> (stub) AST-level source analysis for dangerous patterns
6. BEHAVIORAL  -> (stub) Runtime monitoring during test execution
```

Stages 1--4 enforced. Stages 5--6 return "approved" until implemented.

### Audit types

```rust
pub enum AuditVerdict {
    Approved,
    Rejected { stage: String, reason: String },
    PendingApproval { reason: String },
}

pub struct AuditReport {
    pub plugin_name: String,
    pub plugin_version: String,
    pub origin: PluginOrigin,
    pub stages: Vec<StageResult>,
    pub verdict: AuditVerdict,
    pub timestamp: DateTime<Utc>,
}

pub enum PluginOrigin {
    Agent,
    User,
    System,
}
```

Every install attempt produces an audit report stored in PostgreSQL.

---

## 7. Registry

### Database schema

```sql
CREATE TYPE plugin_origin AS ENUM ('system', 'agent', 'user');
CREATE TYPE plugin_scope AS ENUM ('system', 'user', 'workspace');
CREATE TYPE plugin_status AS ENUM ('installed', 'disabled', 'failed');

CREATE TABLE plugins (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    description TEXT,
    origin plugin_origin NOT NULL,
    scope plugin_scope NOT NULL,
    scope_owner_id UUID,
    status plugin_status NOT NULL DEFAULT 'installed',
    capabilities JSONB NOT NULL,
    wasm_hash TEXT NOT NULL,
    source_repo TEXT,
    source_commit TEXT,
    installed_by UUID REFERENCES users(id),
    installed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(name, scope, scope_owner_id)
);

CREATE TABLE plugin_audit_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plugin_name TEXT NOT NULL,
    plugin_version TEXT NOT NULL,
    origin plugin_origin NOT NULL,
    stages JSONB NOT NULL,
    verdict TEXT NOT NULL,
    rejection_reason TEXT,
    audited_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    audited_by UUID REFERENCES users(id)
);
```

### Registry API

```rust
pub struct PluginRegistry {
    db: Arc<dyn PluginRepo>,
}

impl PluginRegistry {
    pub async fn install(&self, request: InstallRequest) -> Result<AuditReport, PluginError>;
    pub async fn uninstall(&self, name: &str, scope: PluginScope) -> Result<(), PluginError>;
    pub async fn enable(&self, name: &str, scope: PluginScope) -> Result<(), PluginError>;
    pub async fn disable(&self, name: &str, scope: PluginScope) -> Result<(), PluginError>;
    pub async fn list(&self, scope: PluginScope) -> Result<Vec<PluginInfo>, PluginError>;
    pub async fn resolve(&self, name: &str, context: &PluginContext) -> Result<PluginHost, PluginError>;
}
```

---

## 8. sober-plugin-gen

### Two modes

**Template scaffolding** (no LLM):

```
sober plugin new my-plugin --lang rust
sober plugin new my-plugin --lang typescript
```

Produces a valid, compilable skeleton with empty `plugin.toml`, PDK trait impl,
and empty `#[cfg(test)]` block. User writes the logic manually.

**LLM-powered generation:**

```rust
pub struct GenerateRequest {
    pub description: String,
    pub suggested_scope: PluginScope,
    pub capabilities: Vec<Capability>,
    pub origin: PluginOrigin,
}

pub struct PluginGenerator {
    llm: Arc<dyn LlmEngine>,
}

impl PluginGenerator {
    /// Generate plugin source + tests, compile, verify, return artifact.
    pub async fn generate(&self, request: GenerateRequest) -> Result<GenerateResult, GenError>;

    /// Scaffold template only, no LLM.
    pub async fn scaffold(&self, name: &str, lang: Language) -> Result<PathBuf, GenError>;
}
```

### Generation pipeline

```
1. GENERATE   -> LLM produces source + tests from description
                 (prompt includes PDK trait, capability API, plugin.toml format)
2. VALIDATE   -> Parse generated source, verify structural correctness
3. COMPILE    -> Build to WASM via Extism PDK toolchain
4. TEST       -> Run embedded tests in sandboxed Extism instance
5. PASS?      -> Yes: return GenerateResult
                 No: feed errors back to LLM, retry (max 3 attempts)
6. FAIL       -> Return GenError with last compilation/test errors
```

The LLM is forced into the correct plugin format --- the prompt provides the
PDK trait and manifest structure. Generation produces both implementation and
test cases. Tests verify the plugin does what the description asked for.

```rust
pub struct GenerateResult {
    pub source_path: PathBuf,
    pub wasm_bytes: Vec<u8>,
    pub manifest: PluginManifest,
    pub test_results: TestResults,
}
```

---

## 9. Error Types

```rust
// sober-plugin
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("plugin not found: {0}")]
    NotFound(String),

    #[error("audit rejected: {stage} --- {reason}")]
    AuditRejected { stage: String, reason: String },

    #[error("pending approval: {0}")]
    PendingApproval(String),

    #[error("capability denied: {0}")]
    CapabilityDenied(String),

    #[error("plugin execution failed: {0}")]
    ExecutionFailed(String),

    #[error("compilation failed: {0}")]
    CompilationFailed(String),

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

---

## 10. Dependencies

### sober-plugin

| Crate | Purpose |
|-------|---------|
| `sober-core` | Tool trait, shared types, config |
| `sober-sandbox` | Pre-install test execution |
| `extism` | WASM plugin runtime |
| `serde` / `toml` | Manifest parsing |
| `tracing` | Structured logging |
| `thiserror` | Error types |
| `schemars` | JSON Schema generation from Rust types |

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
sober-agent -----> sober-plugin-gen   (trigger generation)
            -----> sober-plugin       (register + execute plugins)

sober-cli   -----> sober-plugin-gen   (sober plugin new / generate / build)
            -----> sober-plugin       (sober plugin install / list / remove)

sober-plugin-gen -> sober-llm         (LLM-powered generation)
                 -> sober-plugin      (compile + test via Extism host)
                 -> sober-core        (shared types)

sober-plugin -----> sober-sandbox     (pre-install test execution)
             -----> sober-core        (Tool trait, shared types)
```

---

## 11. Impact on Existing Designs

| Design | Change |
|--------|--------|
| **001 v1-design** | No change --- sober-plugin remains a stub in v1 |
| **003 sober-core** | Add `PluginRepo` trait to repository traits |
| **005 sober-db** | Add `PgPluginRepo` implementing `PluginRepo` |
| **009 sober-sandbox** | No change --- already documents plugin sandbox path |
| **012 sober-agent** | Post-v1: agent gains ability to trigger plugin generation |
| **014 sober-cli** | Post-v1: CLI gains `sober plugin {new,generate,build,install,list,remove}` |
| **ARCHITECTURE.md** | Add `sober-plugin-gen` to crate table and dependency flow |
