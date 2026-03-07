# 009 --- Granular Execution Sandboxing

**Date:** 2026-03-06

---

## Overview

A unified sandbox layer that all process execution passes through, providing
configurable filesystem, network, PID, and IPC isolation. Two complementary
mechanisms:

- **bwrap (bubblewrap)** --- process-level sandboxing for agent-generated code,
  tool invocations, MCP server processes, and artifact execution. Uses Linux
  namespaces for isolation.
- **wasmtime** (runtime TBD --- raw wasmtime vs Extism) --- structured plugin
  sandboxing with typed API contracts. Supports Rust and TypeScript plugins.
  TypeScript compilation path to be evaluated at implementation time
  (Javy, AssemblyScript, or Extism JS PDK).

Policy is configured via named profiles with per-workspace overrides.

---

## 1. Sandbox Flow

```
Execution request
  -> Resolve sandbox policy (tool override -> workspace config -> user config -> system default)
  -> Select mechanism (bwrap for processes, wasmtime for plugins)
  -> Apply restrictions
  -> Execute
  -> Collect result + audit log
```

---

## 2. Sandbox Profiles

Three built-in profiles, from most to least restrictive:

| Profile | Filesystem | Network | Process | Use case |
|---------|-----------|---------|---------|----------|
| `locked-down` | Read-only workspace src, write only to `/tmp` | None (loopback only) | No spawning | Agent-generated scripts, untrusted artifacts |
| `standard` | Read-only workspace, write to designated output dirs | Allowed domains only (via proxy) | Limited | MCP servers, trusted tools |
| `unrestricted` | Full workspace read/write | Full access | Full | User-initiated commands, development tools |

System default is `standard`. Each profile defines the full set of capabilities
--- no implicit inheritance between profiles.

### User-defined profiles

Users can define custom named profiles alongside the built-ins:

```toml
[sandbox.profiles.ci-runner]
fs_read = ["/workspace/**"]
fs_write = ["/workspace/build/**", "/tmp"]
net_allow = ["registry.npmjs.org", "github.com"]
process_spawn = true
max_execution_seconds = 120
```

### Per-workspace overrides

In `.sober/config.toml`:

```toml
[sandbox]
profile = "standard"

# Override specific capabilities
[sandbox.overrides]
fs_write = ["/workspace/output/**", "/workspace/build/**"]
net_allow = ["api.openai.com", "registry.npmjs.org"]
net_deny = []                    # explicit deny takes precedence over allow
process_spawn = true
max_execution_seconds = 30

# Per-tool overrides (optional, inherit workspace profile if unset)
[sandbox.tools.web_search]
profile = "standard"
net_allow = ["*"]                # search tool needs broad network

[sandbox.tools.code_runner]
profile = "locked-down"
```

Per-tool overrides are optional --- if absent, the tool inherits the workspace
sandbox profile.

---

## 3. Rust Types

### SandboxProfile

```rust
pub enum SandboxProfile {
    LockedDown,
    Standard,
    Unrestricted,
    Custom(String),
}
```

Built-in variants are type-safe. `Custom(String)` references user-defined
profiles by name.

### SandboxPolicy

```rust
pub struct SandboxPolicy {
    pub name: String,               // resolved profile name (for audit logging)
    pub fs_read: Vec<PathBuf>,      // read-only bind mounts
    pub fs_write: Vec<PathBuf>,     // read-write bind mounts
    pub fs_deny: Vec<PathBuf>,      // bind /dev/null over these
    pub net_mode: NetMode,
    pub max_execution_seconds: u32,
    pub allow_spawn: bool,
}

pub enum NetMode {
    None,                           // --unshare-net, loopback only
    AllowedDomains(Vec<String>),    // --unshare-net + socat proxy + domain filter
    Full,                           // no network namespace restriction
}
```

### Profile resolution

```rust
impl SandboxProfile {
    pub fn resolve(&self, profiles: &HashMap<String, SandboxPolicy>) -> SandboxPolicy {
        match self {
            Self::LockedDown => SandboxPolicy {
                name: "locked-down".into(),
                fs_read: vec![],
                fs_write: vec![PathBuf::from("/tmp")],
                fs_deny: vec![],
                net_mode: NetMode::None,
                max_execution_seconds: 30,
                allow_spawn: false,
            },
            Self::Standard => SandboxPolicy {
                name: "standard".into(),
                fs_read: vec![],
                fs_write: vec![],
                fs_deny: vec![],
                net_mode: NetMode::AllowedDomains(vec![]),
                max_execution_seconds: 60,
                allow_spawn: false,
            },
            Self::Unrestricted => SandboxPolicy {
                name: "unrestricted".into(),
                fs_read: vec![],
                fs_write: vec![],
                fs_deny: vec![],
                net_mode: NetMode::Full,
                max_execution_seconds: 300,
                allow_spawn: true,
            },
            Self::Custom(name) => profiles
                .get(name)
                .cloned()
                .expect("custom profile must exist"),
        }
    }
}
```

Built-in profiles provide hardcoded defaults. `fs_read` and `fs_write` for
built-ins are populated at resolution time from workspace context (workspace
root, output directories, etc.).

---

## 4. bwrap Implementation

### How bwrap works

Bubblewrap creates a new mount namespace with an empty tmpfs root, then
selectively bind-mounts only what the sandboxed process needs:

```bash
bwrap \
  --unshare-all \                    # isolate all namespaces (PID, net, mount, IPC, UTS)
  --die-with-parent \                # kill sandbox if parent dies
  --ro-bind /usr /usr \              # read-only system binaries
  --ro-bind /lib /lib \              # read-only libraries
  --ro-bind /lib64 /lib64 \
  --proc /proc \
  --dev /dev \
  --tmpfs /tmp \
  --bind /workspace/output /output \ # read-write for output only
  --ro-bind /workspace/src /src \    # read-only source
  --unshare-net \                    # no network (separate namespace, loopback only)
  -- /usr/bin/python3 script.py
```

### Sandbox builder

A Rust struct that assembles bwrap arguments from the resolved policy:

```rust
pub struct BwrapSandbox {
    policy: SandboxPolicy,
}

impl BwrapSandbox {
    pub fn new(policy: SandboxPolicy) -> Self;

    /// Run a command to completion and return its output.
    pub async fn execute(
        &self,
        command: &[String],
        env: &HashMap<String, String>,
    ) -> Result<SandboxResult, SandboxError>;

    /// Spawn a long-running sandboxed process with piped stdin/stdout.
    /// Used by sober-mcp for MCP server processes that communicate over stdio.
    pub async fn spawn(
        &self,
        command: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Child, SandboxError>;
}

pub struct SandboxResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub denied_network_requests: Vec<String>,
}
```

### Policy resolution order

```
tool-level override (if set)
  -> workspace .sober/config.toml
    -> user ~/.sober/config.toml
      -> system default (standard)
```

Most specific wins. Overrides replace individual fields, not the whole profile.

### Network filtering (AllowedDomains mode)

1. bwrap launches with `--unshare-net` (loopback only inside sandbox)
2. Host-side: a custom HTTPS CONNECT proxy (built with `hyper`) listens on a
   Unix domain socket outside the sandbox
3. socat bridges the host-side UDS into the bwrap namespace, exposing it as a
   TCP port on the sandbox's loopback interface
4. Inside sandbox: `HTTP_PROXY` / `HTTPS_PROXY` env vars point to the bridged
   loopback port
5. Proxy checks each request against the domain allowlist/denylist, rejects
   unauthorized domains

### Proxy implementation details

The proxy is a lightweight HTTP proxy built with `hyper`:

- **Listens on a Unix domain socket** on the host filesystem (not a TCP port).
  This avoids exposing the proxy to non-sandboxed processes.
- **socat bridging:** `socat TCP-LISTEN:<port>,bind=127.0.0.1,fork,reuseaddr
  UNIX-CONNECT:<host-side-uds>` runs inside the bwrap namespace (passed as
  part of the sandbox init), bridging sandbox loopback to the host UDS.
- **HTTPS CONNECT handling:** The proxy reads the `CONNECT` request's host
  header, checks the domain against the allowlist. If allowed, responds with
  `200 Connection Established` and tunnels bytes transparently (no TLS
  termination). If denied, responds with `403 Forbidden` and logs the attempt.
- **Plain HTTP:** For non-CONNECT requests, the proxy checks the `Host` header
  against the allowlist before forwarding.
- **Non-HTTP traffic is blocked:** Only TCP connections through the HTTP proxy
  are permitted. The sandbox has no direct network access (loopback only via
  `--unshare-net`), so all external traffic must go through the proxy.
- **Connection lifecycle:** Proxy starts before bwrap, shuts down after the
  sandboxed process exits. Denied request domains are collected and returned
  in the `SandboxResult`.

### Process lifecycle

```
1. Resolve SandboxPolicy from config chain
2. If NetMode::AllowedDomains -> start socat + proxy on host side
3. Build bwrap arg list from policy
4. Spawn: bwrap [...args] -- <command>
5. Stream stdout/stderr back to caller
6. On timeout (max_execution_seconds) -> SIGTERM, then SIGKILL after 5s grace
7. Collect exit code, tear down proxy if started
8. Audit log: policy used, command, duration, exit code, any denied network requests
```

### Always applied (regardless of profile)

- `--die-with-parent` --- sandbox dies if sober process dies
- `--unshare-pid` --- sandboxed process can't see host PIDs
- `/dev/null` bound over `~/.ssh`, `~/.aws`, `~/.gnupg`, `.env` files
- `--new-session` --- prevent TTY escape via TIOCSTI

### Performance

bwrap is ~3x faster than Docker for process spawning (no daemon round-trip).
100 echo commands: 0.37s (bwrap) vs 1.13s (Docker). This matters when the
agent spawns many short-lived executions.

---

## 5. wasmtime for Plugins

Plugins use wasmtime (or potentially Extism) for structured sandboxing with
typed API contracts. This is a separate mechanism from bwrap --- plugins
implement the `SoberPlugin` trait, declare capabilities, and communicate via
a defined host/guest interface.

**Why both bwrap and wasmtime:**

| Aspect | wasmtime (plugins) | bwrap (processes) |
|--------|-------------------|-------------------|
| Use case | Structured plugins with typed API | Arbitrary code, scripts, tools |
| Isolation | Capability-based (formal sandbox) | Namespace-based (kernel isolation) |
| Language | Rust, TypeScript (via WASM) | Any language, any binary |
| API | Typed host/guest functions | stdin/stdout/socket (black box) |
| Escape risk | Very low (WASM formal model) | Low (kernel namespaces) |

**TypeScript plugin support:** To be evaluated at implementation time. Options
include Javy (QuickJS in WASM), AssemblyScript (TS-like subset), or Extism
JS PDK. The choice depends on runtime performance needs and developer
experience goals.

---

## 6. Audit & Observability

### Audit log

Every sandboxed execution produces an audit record:

```rust
pub struct SandboxAuditEntry {
    pub execution_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub workspace_id: Option<WorkspaceId>,
    pub user_id: Option<UserId>,
    pub policy: SandboxPolicy,         // snapshot of resolved policy
    pub command: Vec<String>,          // what was executed
    pub trigger: ExecutionTrigger,     // agent, tool, user, scheduler
    pub duration_ms: u64,
    pub exit_code: Option<i32>,
    pub denied_network_requests: Vec<String>,
    pub outcome: ExecutionOutcome,
}

pub enum ExecutionTrigger {
    Agent,
    Tool(String),
    User,
    Scheduler,
}

pub enum ExecutionOutcome {
    Success,
    Timeout,
    Killed,
    Error(String),
}
```

Stored in PostgreSQL. Scheduler prunes entries past retention window (default
90 days). Traces in `.sober/traces/` for per-workspace debugging.

### Prometheus metrics

Operational metrics exposed via the system's metrics endpoint:

- Sandbox executions counter (by profile, trigger, outcome)
- Execution duration histogram
- Active sandboxes gauge
- Denied network requests counter
- Proxy connection pool usage

Metric definitions deferred to `sober-api` metrics implementation.

---

## 7. Crate: `sober-sandbox`

### Responsibilities

- `SandboxPolicy` / `SandboxProfile` types and resolution
- `BwrapSandbox` builder and process spawning
- Socat proxy lifecycle for filtered network mode
- Audit entry generation
- Profile registry (built-in + custom from config)

### Dependency flow

```
sober-agent  --> sober-sandbox   (sandbox artifact/tool execution)
sober-mcp    --> sober-sandbox   (sandbox MCP server processes)
sober-plugin --> sober-sandbox   (bwrap for pre-WASM audit runs)
sober-sandbox --> sober-core     (shared types, config)
```

`sober-sandbox` sits at the same level as `sober-crypto` and `sober-memory`
--- a utility crate that higher-level crates depend on. It does not depend on
`sober-agent`, `sober-api`, or `sober-plugin`.

### External runtime dependencies

- `bwrap` --- bubblewrap binary (widely packaged, used by Flatpak)
- `socat` --- socket relay for network proxy bridging

### Crate dependencies

- `sober-core` --- shared types, config
- `tokio` --- process management, timeouts
- `serde` / `toml` --- policy deserialization from config
- `tracing` --- structured logging

### Implementation order

`sober-sandbox` depends only on `sober-core`. It must be implemented **before
sober-agent (012)** since agent tool execution and MCP server spawning go
through the sandbox. Natural slot: between phases 2 and 5 in the bootstrap
order --- alongside other utility crates (sober-crypto, sober-memory).

---

## 8. Impact on Existing Designs

| Design | Change |
|--------|--------|
| **000 bootstrap-gaps** | Add `sober-sandbox` to crate map and bootstrap order. Note `bwrap` and `socat` as runtime dependencies. |
| **001 v1-design** | Add `sober-sandbox` to crate table and dependency flow. Move "sandboxed code execution" from deferred to included (process sandbox via bwrap). Plugin/WASM sandbox remains deferred. |
| **012 sober-agent** | Tool execution and agent-generated code go through `sober-sandbox`. `McpClient::connect` spawns MCP servers via sandbox. |
| **017 workspaces** | `.sober/config.toml` gains `[sandbox]` section for profile and overrides. |
| **ARCHITECTURE.md** | Add `sober-sandbox` to crate table and system diagram. |
