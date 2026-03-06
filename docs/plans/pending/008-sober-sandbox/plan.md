# 008 --- sober-sandbox: Implementation Plan

**Date:** 2026-03-06
**Status:** Pending
**Depends on:** 003 (sober-core)
**Must complete before:** 010 (sober-mcp), 011 (sober-agent)

---

## Steps

### 1. Crate scaffold

Create `backend/crates/sober-sandbox/` with `Cargo.toml`:

- `sober-core = { path = "../sober-core" }`
- `tokio = { version = "1", features = ["process", "time", "io-util"] }`
- `serde = { version = "1", features = ["derive"] }`
- `toml = "0.8"`
- `tracing = "0.1"`
- `uuid = { version = "1", features = ["v7"] }`
- `chrono = { version = "0.4", features = ["serde"] }`
- `thiserror = "2"`

Add `"crates/sober-sandbox"` to the workspace members in `backend/Cargo.toml`.

### 2. Module structure

```
backend/crates/sober-sandbox/src/
  lib.rs          # Public API, module declarations, re-exports
  error.rs        # SandboxError enum
  policy.rs       # SandboxPolicy, SandboxProfile, NetMode types
  config.rs       # SandboxConfig deserialization from TOML
  resolve.rs      # Policy resolution chain (tool -> workspace -> user -> default)
  bwrap.rs        # BwrapSandbox builder and process execution
  proxy.rs        # Socat proxy lifecycle for AllowedDomains network mode
  audit.rs        # SandboxAuditEntry, ExecutionTrigger, ExecutionOutcome
  detect.rs       # Runtime detection of bwrap/socat binaries
```

### 3. Implement `error.rs`

Define `SandboxError` enum:

- `BwrapNotFound` --- bwrap binary not on PATH
- `SocatNotFound` --- socat binary not on PATH (only when AllowedDomains needed)
- `SpawnFailed(String)` --- bwrap process failed to start
- `Timeout { seconds: u32 }` --- execution exceeded max_execution_seconds
- `ProxyFailed(String)` --- socat/proxy setup failed
- `PolicyResolutionFailed(String)` --- config parse or profile lookup error
- `Killed` --- process was killed (SIGKILL after grace period)

Derive `Debug`, `thiserror::Error`. Implement `From<SandboxError>` for `AppError`
(maps to `AppError::Internal`).

### 4. Implement `policy.rs`

Define core types from the design:

- `SandboxProfile` enum: `LockedDown`, `Standard`, `Unrestricted`, `Custom(String)`.
- `SandboxPolicy` struct with all fields from design.
- `NetMode` enum: `None`, `AllowedDomains(Vec<String>)`, `Full`.
- `SandboxProfile::resolve()` method that expands built-in profiles to concrete
  `SandboxPolicy` values and looks up `Custom` profiles from a registry.
- Derive `Debug`, `Clone`, `Serialize`, `Deserialize` on all types.
- `SandboxProfile` deserialization: parse `"locked-down"`, `"standard"`,
  `"unrestricted"` as built-in variants, anything else as `Custom(name)`.

### 5. Implement `config.rs`

Deserialization structs matching the TOML config format:

- `SandboxConfig` --- top-level `[sandbox]` section.
  - `profile: SandboxProfile` (default: `Standard`)
  - `overrides: Option<SandboxOverrides>` --- per-workspace field overrides
  - `profiles: HashMap<String, SandboxPolicyConfig>` --- user-defined profiles
  - `tools: HashMap<String, ToolSandboxConfig>` --- per-tool overrides
- `SandboxOverrides` --- optional field-level overrides (fs_write, net_allow, etc.)
- `SandboxPolicyConfig` --- TOML-friendly shape that converts to `SandboxPolicy`.
- `ToolSandboxConfig` --- per-tool profile + overrides.

All fields optional with serde defaults. Parsing errors produce
`SandboxError::PolicyResolutionFailed`.

### 6. Implement `resolve.rs`

Policy resolution chain:

```rust
pub fn resolve_policy(
    tool_name: Option<&str>,
    workspace_config: Option<&SandboxConfig>,
    user_config: Option<&SandboxConfig>,
    custom_profiles: &HashMap<String, SandboxPolicy>,
) -> Result<SandboxPolicy, SandboxError>;
```

Resolution logic:
1. If `tool_name` is set and workspace config has a matching `tools.<name>` entry,
   use that tool's profile as the base.
2. Otherwise, use the workspace config's `profile` field.
3. If no workspace config, fall back to user config.
4. If no user config, fall back to system default (`Standard`).
5. Resolve the profile to a `SandboxPolicy` via `SandboxProfile::resolve()`.
6. Apply any overrides (workspace-level, then tool-level) on top of the resolved policy.

### 7. Implement `detect.rs`

Runtime binary detection:

```rust
pub fn detect_bwrap() -> Result<PathBuf, SandboxError>;
pub fn detect_socat() -> Result<PathBuf, SandboxError>;
```

- Use `which::which("bwrap")` or search PATH manually.
- Cache result after first call (lazy_static or OnceCell).
- Return clear error with installation instructions if not found.
- Log detected binary paths at startup via `tracing::info!`.

### 8. Implement `bwrap.rs`

The core sandbox execution engine:

- `BwrapSandbox::new(policy: SandboxPolicy) -> Self`
- `BwrapSandbox::execute(command, env) -> Result<SandboxResult, SandboxError>`
- `BwrapSandbox::spawn(command, env) -> Result<Child, SandboxError>` --- long-running
  variant that returns a `tokio::process::Child` with piped stdin/stdout instead of
  waiting for completion. Required by `sober-mcp` for MCP server processes.

`execute` implementation:
1. Build bwrap argument list from policy:
   - Always: `--unshare-pid`, `--die-with-parent`, `--new-session`
   - Always: `--proc /proc`, `--dev /dev`
   - Always: system lib bind-mounts (`/usr`, `/lib`, `/lib64`, `/bin`, `/sbin`) as `--ro-bind`
   - Always: `--tmpfs /tmp`
   - Always: deny sensitive paths (`~/.ssh`, `~/.aws`, `~/.gnupg`) via `--bind /dev/null`
   - Per-policy: `fs_read` paths as `--ro-bind`
   - Per-policy: `fs_write` paths as `--bind`
   - Per-policy: `fs_deny` paths as `--ro-bind /dev/null`
   - Per-policy: if `net_mode` is `None` or `AllowedDomains`, add `--unshare-net`
   - Per-policy: if `!allow_spawn`, add seccomp filter (stretch goal --- initial impl can skip)
2. If `NetMode::AllowedDomains`, start proxy via `proxy.rs` before spawning.
3. Spawn bwrap as `tokio::process::Command`.
4. Set `HTTP_PROXY`/`HTTPS_PROXY` env vars if proxy is active.
5. Apply timeout via `tokio::time::timeout`.
6. On timeout: send SIGTERM, wait 5s, then SIGKILL.
7. Collect stdout, stderr, exit code into `SandboxResult`.
8. Tear down proxy if started.

### 9. Implement `proxy.rs`

Socat proxy lifecycle for `NetMode::AllowedDomains`:

- `ProxyBridge::start(allowed_domains, denied_domains) -> Result<ProxyBridge, SandboxError>`
- `ProxyBridge::port() -> u16` --- the port inside the sandbox
- `ProxyBridge::stop(self) -> Result<Vec<String>, SandboxError>` --- returns denied request log

Implementation:
1. Start a lightweight HTTP proxy (consider `hyper` or a simple TCP forwarder).
2. Start socat to bridge from sandbox loopback to the proxy Unix socket.
3. Proxy inspects CONNECT requests, checks domain against allowlist/denylist.
4. Denied domains are logged and the connection is rejected.
5. On `stop`, kill socat process and return the denied request log.

**Note:** The proxy implementation is the most complex part. For the initial
version, a simple domain-checking CONNECT proxy is sufficient. Full SOCKS5
support can be added later.

### 10. Implement `audit.rs`

Audit entry types from the design:

- `SandboxAuditEntry` struct with all fields.
- `ExecutionTrigger` enum: `Agent`, `Tool(String)`, `User`, `Scheduler`.
- `ExecutionOutcome` enum: `Success`, `Timeout`, `Killed`, `Error(String)`.
- `SandboxAuditEntry::from_result(policy, command, trigger, result) -> Self`
  --- construct an entry from execution inputs and result.

Derive `Debug`, `Clone`, `Serialize`, `Deserialize` on all types. The actual
storage (PostgreSQL insert) is handled by the calling crate (`sober-agent` or
`sober-api`), not by `sober-sandbox` itself. This crate only produces the
audit data structure.

### 11. Wire up `lib.rs`

- Declare all modules.
- Re-export key types:
  - `pub use policy::{SandboxPolicy, SandboxProfile, NetMode};`
  - `pub use config::SandboxConfig;`
  - `pub use resolve::resolve_policy;`
  - `pub use bwrap::{BwrapSandbox, SandboxResult};`
  - `pub use audit::{SandboxAuditEntry, ExecutionTrigger, ExecutionOutcome};`
  - `pub use error::SandboxError;`
- Add a startup check function:
  ```rust
  pub fn check_runtime_deps() -> Result<(), SandboxError>;
  ```
  Calls `detect_bwrap()` and optionally `detect_socat()`. Intended to be called
  at application startup to fail fast if dependencies are missing.

### 12. Tests

Unit tests:

- **Policy resolution:** Verify built-in profile defaults. Verify custom profile
  lookup. Verify override application (workspace overrides modify specific fields
  without replacing the whole policy). Verify tool-level overrides take precedence.
- **Config parsing:** Parse sample TOML configs into `SandboxConfig`. Verify
  defaults. Verify invalid configs produce `PolicyResolutionFailed`.
- **Bwrap arg builder:** Verify that `BwrapSandbox` produces the correct argument
  list for each profile. Test `locked-down` produces `--unshare-net`, `standard`
  with allowed domains produces proxy setup, `unrestricted` omits network
  namespace flags. Verify sensitive path denial is always present.
- **Audit entry construction:** Build entries from various outcomes, verify
  serialization roundtrips.

Integration tests (require bwrap installed):

- **Basic execution:** Run `echo hello` in a locked-down sandbox, verify stdout.
- **Filesystem isolation:** Write to a path not in `fs_write`, verify failure.
  Write to an allowed path, verify success.
- **Network isolation:** Run `curl` in a `NetMode::None` sandbox, verify failure.
- **Timeout:** Run `sleep 60` with a 2-second timeout, verify `Timeout` outcome.
- **Sensitive path denial:** Attempt to read `~/.ssh/id_rsa` from sandbox,
  verify it reads as empty (`/dev/null`).

Skip integration tests in CI if bwrap is not available (detect and `#[ignore]`
or use a `cfg` flag).

### 13. Verification

```bash
cargo clippy -p sober-sandbox -- -D warnings
cargo test -p sober-sandbox
cargo doc -p sober-sandbox --no-deps
```

---

## Acceptance Criteria

- [ ] `SandboxPolicy`, `SandboxProfile`, `NetMode` types compile and are usable from downstream crates.
- [ ] Policy resolution chain correctly applies tool -> workspace -> user -> default precedence.
- [ ] `BwrapSandbox` produces correct bwrap argument lists for all three built-in profiles.
- [ ] Basic bwrap execution works: run a command, get stdout/stderr/exit code.
- [ ] Filesystem isolation enforced: writes outside allowed paths fail.
- [ ] Network isolation enforced: `NetMode::None` blocks all external access.
- [ ] Timeout enforcement: processes killed after `max_execution_seconds`.
- [ ] Sensitive paths (`~/.ssh`, `~/.aws`, `~/.gnupg`) always denied.
- [ ] `SandboxConfig` parses from TOML with correct defaults.
- [ ] `SandboxAuditEntry` captures all execution metadata.
- [ ] Runtime dependency detection fails fast with clear error if bwrap is missing.
- [ ] `cargo clippy -p sober-sandbox -- -D warnings` reports zero warnings.
- [ ] `cargo doc -p sober-sandbox --no-deps` generates documentation without warnings.
- [ ] No `.unwrap()` in library code (only in tests).
- [ ] All public items have doc comments.
