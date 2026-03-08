# sober-plugin & sober-plugin-gen Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a WASM-based plugin system with capability-driven isolation (Extism), a progressive audit pipeline, a plugin registry, and an LLM-powered plugin generation crate.

**Architecture:** Two crates. `sober-plugin` handles runtime concerns (registry, Extism host, capability enforcement, audit pipeline). `sober-plugin-gen` handles generation concerns (template scaffolding, LLM-powered source generation with test verification, compilation to WASM).

**Tech Stack:** Rust, Extism (wasmtime-based), serde/toml, schemars, thiserror, tracing. `sober-plugin-gen` additionally depends on `sober-llm`.

**Design doc:** `docs/plans/pending/019-sober-plugin/design.md`

**Priority:** Post-v1. Nothing on the v1 critical path depends on these crates.

---

## Prerequisites

- `sober-core` (003) must be implemented: `Tool` trait, `AppError`, ID newtypes, config types.
- `sober-db` (005) must be implemented: pool creation, migration infrastructure.
- `sober-sandbox` (009) must be implemented: `BwrapSandbox` for pre-install test execution.
- `sober-llm` (008) must be implemented: `LlmEngine` trait (required by `sober-plugin-gen`).

---

## Phase 1: sober-plugin

### Task 1: Scaffold sober-plugin crate

**Files:**
- Create: `backend/crates/sober-plugin/Cargo.toml`
- Create: `backend/crates/sober-plugin/src/lib.rs`
- Modify: `backend/Cargo.toml` (add `sober-plugin` to workspace members)

**Step 1: Create the crate directory**

```bash
mkdir -p backend/crates/sober-plugin/src
```

**Step 2: Create Cargo.toml**

```toml
[package]
name = "sober-plugin"
version = "0.1.0"
edition = "2024"

[dependencies]
sober-core = { path = "../sober-core" }
sober-sandbox = { path = "../sober-sandbox" }
extism = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
schemars = "1"
tracing = "0.1"
thiserror = "2"
uuid = { version = "1", features = ["v7"] }
chrono = { version = "0.4", features = ["serde"] }
async-trait = "0.1"
tokio = { version = "1", features = ["fs"] }
anyhow = "1"
```

**Step 3: Create lib.rs with module declarations**

Declare modules (initially empty files):

```rust
pub mod error;
pub mod capability;
pub mod manifest;
pub mod host;
pub mod audit;
pub mod registry;
```

**Step 4: Add to workspace**

Add `"crates/sober-plugin"` to `backend/Cargo.toml` workspace members.

**Verify:** `cargo check -p sober-plugin` compiles.

---

### Task 2: Error types and capabilities

**Files:**
- Create: `backend/crates/sober-plugin/src/error.rs`
- Create: `backend/crates/sober-plugin/src/capability.rs`

**Step 1: Implement `error.rs`**

Define `PluginError` enum:

- `NotFound(String)` --- plugin not in registry
- `AuditRejected { stage: String, reason: String }` --- audit pipeline rejected
- `PendingApproval(String)` --- requires human sign-off
- `CapabilityDenied(String)` --- plugin tried to use undeclared capability
- `ExecutionFailed(String)` --- runtime execution error
- `CompilationFailed(String)` --- WASM compilation error
- `ManifestInvalid(String)` --- plugin.toml parse or validation error
- `Internal(anyhow::Error)` --- transparent from anyhow

Derive `Debug`, `thiserror::Error`. Implement `From<PluginError>` for `AppError`.

**Step 2: Implement `capability.rs`**

Define `Capability` enum:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Capability {
    MemoryRead(Vec<ScopeKind>),
    MemoryWrite(Vec<ScopeKind>),
    Network(Vec<String>),
    Filesystem(Vec<PathBuf>),
    LlmCall,
    ToolCall(Vec<String>),
}
```

Add a helper method:

```rust
impl Capability {
    /// Check if `self` is a subset of `other` (for auto-approval checks).
    pub fn is_subset_of(&self, available: &[Capability]) -> bool;
}
```

**Verify:** `cargo check -p sober-plugin`

---

### Task 3: Plugin manifest

**Files:**
- Create: `backend/crates/sober-plugin/src/manifest.rs`

Define manifest types matching `plugin.toml` format:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    pub capabilities: CapabilitiesConfig,
    pub tools: Vec<ToolEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    pub description: String,
    pub origin: PluginOrigin,
    pub scope: PluginScope,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginOrigin { System, Agent, User }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginScope { System, User, Workspace }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesConfig {
    #[serde(default)]
    pub memory_read: Vec<String>,
    #[serde(default)]
    pub memory_write: Vec<String>,
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub filesystem: Vec<String>,
    #[serde(default)]
    pub llm_call: bool,
    #[serde(default)]
    pub tool_call: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEntry {
    pub name: String,
    pub description: String,
}
```

Add methods:

- `PluginManifest::from_toml(content: &str) -> Result<Self, PluginError>` --- parse and validate
- `PluginManifest::capabilities(&self) -> Vec<Capability>` --- convert config to typed capabilities
- `PluginManifest::validate(&self) -> Result<(), PluginError>` --- check name format, version, etc.

Unit tests: parse sample plugin.toml strings, verify roundtrip, verify validation
catches invalid names/versions.

**Verify:** `cargo test -p sober-plugin`

---

### Task 4: Extism plugin host

**Files:**
- Create: `backend/crates/sober-plugin/src/host.rs`

Implement `PluginHost` --- the Extism wrapper that loads WASM and wires host
functions based on declared capabilities.

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

**Step 1: Define host functions**

Each host function is registered with Extism's `PluginBuilder::with_function`:

- `host_memory_read` --- wired when `MemoryRead` capability present
- `host_memory_write` --- wired when `MemoryWrite` capability present
- `host_http_request` --- wired when `Network` capability present. Use Extism's
  `allowed_hosts` manifest field for domain filtering.
- `host_llm_complete` --- wired when `LlmCall` capability present
- `host_call_tool` --- wired when `ToolCall` capability present

For the initial implementation, host functions can be stubs that return errors
with "not yet connected" messages. The real implementations require runtime
integration with sober-memory, sober-llm, etc. The important thing is that
the wiring logic is correct --- functions are only registered when the
capability is declared.

**Step 2: Implement `PluginTool`**

```rust
pub struct PluginTool {
    host: Arc<Mutex<PluginHost>>,
    tool_entry: String,
    metadata: ToolMetadata,
}

impl Tool for PluginTool {
    fn metadata(&self) -> ToolMetadata { ... }
    async fn execute(&self, input: Value) -> Result<ToolOutput, ToolError> { ... }
}
```

Each `[[tools]]` entry in the manifest becomes a `PluginTool`. The agent sees
these identically to MCP tools or built-in tools.

**Verify:** `cargo check -p sober-plugin`. Write a unit test that loads a
trivial WASM plugin (e.g., count_vowels from Extism examples) and calls it.

---

### Task 5: Audit pipeline

**Files:**
- Create: `backend/crates/sober-plugin/src/audit.rs`

Implement the progressive audit pipeline.

```rust
pub struct AuditPipeline;

impl AuditPipeline {
    /// Run all audit stages on a plugin.
    pub async fn audit(
        &self,
        source_path: Option<&Path>,
        wasm_bytes: &[u8],
        manifest: &PluginManifest,
    ) -> Result<AuditReport, PluginError>;
}
```

**Stages (implement in order):**

1. **Validate** --- check manifest structure, capability declarations are
   well-formed, tool entries are valid. Implemented fully.

2. **Capability** --- load WASM in Extism with only declared host functions.
   Verify the WASM module loads without import errors. Implemented fully.

3. **Test** --- run embedded `#[cfg(test)]` tests via the Extism instance.
   This requires the plugin to export a test runner function. Implemented fully.

4. **Static** --- stub. Returns `StageResult::Passed` always. Future: AST
   analysis of source for dangerous patterns.

5. **Behavioral** --- stub. Returns `StageResult::Passed` always. Future:
   runtime monitoring during test execution.

**Types:**

```rust
pub struct AuditReport {
    pub plugin_name: String,
    pub plugin_version: String,
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
    PendingApproval { reason: String },
}
```

**Approval logic:**

- System origin: always `Approved`
- Agent origin: `Approved` if all stages pass AND capabilities are a subset of
  agent's existing access. Otherwise `PendingApproval`.
- User origin: `PendingApproval` if all stages pass. `Rejected` if any stage fails.

**Verify:** `cargo test -p sober-plugin`

---

### Task 6: Database schema and repository

**Files:**
- Create: migration in `backend/migrations/` for plugin tables
- Modify: `sober-core` to add `PluginRepo` trait
- Modify: `sober-db` to add `PgPluginRepo`

**Step 1: SQL migration**

Create the `plugins` and `plugin_audit_logs` tables as defined in the design
doc (section 7). Add `plugin_origin`, `plugin_scope`, `plugin_status` enums.

**Step 2: `PluginRepo` trait in sober-core**

```rust
#[async_trait]
pub trait PluginRepo: Send + Sync {
    async fn insert(&self, plugin: &PluginRecord) -> Result<(), AppError>;
    async fn get_by_name(&self, name: &str, scope: PluginScope, owner: Option<Uuid>) -> Result<Option<PluginRecord>, AppError>;
    async fn list_by_scope(&self, scope: PluginScope, owner: Option<Uuid>) -> Result<Vec<PluginRecord>, AppError>;
    async fn update_status(&self, id: Uuid, status: PluginStatus) -> Result<(), AppError>;
    async fn delete(&self, id: Uuid) -> Result<(), AppError>;
    async fn insert_audit_log(&self, log: &PluginAuditRecord) -> Result<(), AppError>;
}
```

**Step 3: `PgPluginRepo` in sober-db**

Implement `PluginRepo` with sqlx queries against the new tables.

**Verify:** `cargo test -p sober-db` (with test database)

---

### Task 7: Plugin registry

**Files:**
- Create: `backend/crates/sober-plugin/src/registry.rs`

Implement `PluginRegistry` --- the public API for managing plugins.

```rust
// Generic over PluginRepo (RPITIT traits are not dyn-compatible)
pub struct PluginRegistry<P: PluginRepo> {
    db: P,
    audit: AuditPipeline,
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

**`install` flow:**
1. Parse manifest from source path
2. Compile source to WASM (if source provided)
3. Run audit pipeline
4. If approved: store WASM artifact, insert DB record, commit source to git
5. If pending: insert DB record with `pending` status, return report
6. If rejected: insert audit log, return report with rejection

**`resolve` flow:**
1. Check workspace plugins, then user plugins, then system plugins
2. Load the first match
3. Return a loaded `PluginHost` ready for execution

**Verify:** `cargo test -p sober-plugin`

---

### Task 8: Wire up lib.rs and integration tests

**Files:**
- Modify: `backend/crates/sober-plugin/src/lib.rs`

Re-export public API:

```rust
pub use capability::Capability;
pub use error::PluginError;
pub use manifest::{PluginManifest, PluginOrigin, PluginScope};
pub use host::{PluginHost, PluginTool};
pub use audit::{AuditPipeline, AuditReport, AuditVerdict};
pub use registry::PluginRegistry;
```

Integration tests:

- Load a trivial WASM plugin, verify tool execution works
- Install a plugin through the registry, verify it appears in `list()`
- Verify capability enforcement: plugin without `Network` capability cannot
  call `host_http_request`
- Verify audit pipeline: valid plugin passes, invalid manifest fails at validate stage

**Verify:**

```bash
cargo clippy -p sober-plugin -- -D warnings
cargo test -p sober-plugin
cargo doc -p sober-plugin --no-deps
```

---

## Phase 2: sober-plugin-gen

### Task 9: Scaffold sober-plugin-gen crate

**Files:**
- Create: `backend/crates/sober-plugin-gen/Cargo.toml`
- Create: `backend/crates/sober-plugin-gen/src/lib.rs`
- Modify: `backend/Cargo.toml` (add `sober-plugin-gen` to workspace members)

```toml
[package]
name = "sober-plugin-gen"
version = "0.1.0"
edition = "2024"

[dependencies]
sober-core = { path = "../sober-core" }
sober-plugin = { path = "../sober-plugin" }
sober-llm = { path = "../sober-llm" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
tracing = "0.1"
thiserror = "2"
tokio = { version = "1", features = ["fs", "process"] }
anyhow = "1"
```

Module structure:

```rust
pub mod error;
pub mod scaffold;
pub mod generate;
pub mod compile;
```

**Verify:** `cargo check -p sober-plugin-gen`

---

### Task 10: Error types and template scaffolding

**Files:**
- Create: `backend/crates/sober-plugin-gen/src/error.rs`
- Create: `backend/crates/sober-plugin-gen/src/scaffold.rs`

**Step 1: Implement `error.rs`**

Define `GenError` enum:

- `GenerationFailed { attempts: u32, reason: String }`
- `CompilationFailed(String)`
- `TestsFailed(String)`
- `ScaffoldFailed(String)`
- `Llm(LlmError)` --- from sober-llm
- `Internal(anyhow::Error)`

**Step 2: Implement `scaffold.rs`**

Template scaffolding for manual plugin authoring:

```rust
pub enum Language { Rust, TypeScript }

pub async fn scaffold(
    name: &str,
    lang: Language,
    output_dir: &Path,
) -> Result<PathBuf, GenError>;
```

Generates:

- `plugin.toml` with placeholder values
- `src/lib.rs` (Rust) or `src/index.ts` (TypeScript) with PDK trait skeleton
  and empty `#[cfg(test)] mod tests`

Templates are embedded in the binary via `include_str!` or string literals.
No external template files.

**Verify:** `cargo test -p sober-plugin-gen` --- scaffold a plugin, verify
the generated files parse correctly.

---

### Task 11: WASM compilation

**Files:**
- Create: `backend/crates/sober-plugin-gen/src/compile.rs`

Compile plugin source to WASM:

```rust
pub async fn compile(
    source_dir: &Path,
    lang: Language,
) -> Result<Vec<u8>, GenError>;
```

**Rust compilation:**
1. Run `cargo build --target wasm32-wasip2 --release` in the plugin source dir
2. Read the output `.wasm` file
3. Return bytes

**TypeScript compilation:**
- Deferred. Return `GenError::CompilationFailed("TypeScript compilation not
  yet supported")` for now. Implementation depends on chosen TS-to-WASM path
  (Javy, AssemblyScript, or Extism JS PDK).

**Verify:** `cargo test -p sober-plugin-gen` --- compile a scaffolded Rust
plugin to WASM (requires `wasm32-wasip2` target installed).

---

### Task 12: LLM-powered generation

**Files:**
- Create: `backend/crates/sober-plugin-gen/src/generate.rs`

The self-correcting generation loop:

```rust
pub struct PluginGenerator {
    llm: Arc<dyn LlmEngine>, // LlmEngine uses #[async_trait], dyn-compatible
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

impl PluginGenerator {
    pub async fn generate(
        &self,
        request: GenerateRequest,
    ) -> Result<GenerateResult, GenError>;
}
```

**Generation pipeline:**

1. Build prompt from request description, PDK trait definition, plugin.toml
   format, and declared capabilities. The prompt forces correct structure.
2. Call LLM to produce plugin source + embedded tests.
3. Parse the generated source --- verify structural correctness.
4. Write source to a temp directory.
5. Compile to WASM via `compile::compile()`.
6. Load in Extism, run tests via `PluginHost`.
7. If compilation or tests fail: feed errors back to LLM as context, retry.
   Maximum 3 attempts.
8. If all 3 attempts fail: return `GenError::GenerationFailed`.
9. On success: return `GenerateResult` with source, WASM, manifest, and
   test results.

**Verify:** Integration test with a mock `LlmEngine` that returns a
pre-written plugin source. Verify the pipeline compiles and tests it.

---

### Task 13: Wire up lib.rs

**Files:**
- Modify: `backend/crates/sober-plugin-gen/src/lib.rs`

Re-export public API:

```rust
pub use error::GenError;
pub use scaffold::{scaffold, Language};
pub use compile::compile;
pub use generate::{PluginGenerator, GenerateRequest, GenerateResult};
```

**Verify:**

```bash
cargo clippy -p sober-plugin-gen -- -D warnings
cargo test -p sober-plugin-gen
cargo doc -p sober-plugin-gen --no-deps
```

---

## Acceptance Criteria

### sober-plugin

- [ ] Extism loads WASM plugins and executes exported functions.
- [ ] Host functions are wired only when matching capability is declared.
- [ ] Plugins without a capability cannot call the corresponding host function.
- [ ] `PluginManifest` parses and validates `plugin.toml` files.
- [ ] `AuditPipeline` runs validate, capability, and test stages; static and behavioral are stubs.
- [ ] Approval thresholds differ by origin (system auto-approved, agent conditional, user always pending).
- [ ] `PluginRegistry` installs, lists, enables, disables, and uninstalls plugins.
- [ ] Plugin resolution follows workspace -> user -> system precedence.
- [ ] `PluginTool` implements the `Tool` trait from sober-core.
- [ ] Database schema for `plugins` and `plugin_audit_logs` tables.
- [ ] `cargo clippy -p sober-plugin -- -D warnings` reports zero warnings.
- [ ] All public items have doc comments.
- [ ] No `.unwrap()` in library code.

### sober-plugin-gen

- [ ] Template scaffolding produces valid, compilable Rust plugin skeletons.
- [ ] TypeScript scaffolding produces valid skeleton (compilation deferred).
- [ ] Rust plugins compile to WASM via `wasm32-wasip2` target.
- [ ] LLM-powered generation produces source + tests from a description.
- [ ] Generation pipeline retries on compilation/test failure (max 3 attempts).
- [ ] Generated plugins have correct `plugin.toml` format and embedded tests.
- [ ] `cargo clippy -p sober-plugin-gen -- -D warnings` reports zero warnings.
- [ ] All public items have doc comments.
- [ ] No `.unwrap()` in library code.
