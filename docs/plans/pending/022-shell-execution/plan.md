# 022 --- Agent Shell Execution: Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Give the agent the ability to execute shell commands in user workspaces with configurable permission modes and a confirmation flow for sensitive commands.

**Architecture:** A `ShellTool` in `sober-agent`'s tool registry delegates to `BwrapSandbox` for sandboxed execution. A `CommandPolicy` in `sober-sandbox` classifies command risk. New proto events and WebSocket messages support an interactive confirmation flow. Frontend adds a confirmation card component and a permission mode toggle in a status bar.

**Tech Stack:** Rust (sober-agent, sober-sandbox, sober-core, sober-api), Protobuf (agent.proto), TypeScript/Svelte 5, Tailwind CSS

**Depends on:** 009 (sober-sandbox), 012 (sober-agent), 013 (sober-api), 015 (frontend), 017 (workspaces --- must be implemented first)

---

## Task 1: Add PermissionMode to sober-core and RiskLevel to sober-sandbox

`PermissionMode` lives in `sober-core` (used by workspace config).
`RiskLevel` lives in `sober-sandbox` (used by CommandPolicy and ShellTool;
both crates already depend on sober-sandbox).

**Files:**
- Modify: `backend/crates/sober-core/src/workspace_config.rs` (add PermissionMode, sandbox/shell config structs)
- Modify: `backend/crates/sober-core/src/lib.rs` (re-export)
- Create: `backend/crates/sober-sandbox/src/risk.rs` (RiskLevel)
- Modify: `backend/crates/sober-sandbox/src/lib.rs` (re-export)

**Step 1: Write failing tests for PermissionMode**

Add to the test module in `workspace_config.rs`:

```rust
#[test]
fn permission_mode_serde_roundtrip() {
    let variants = [PermissionMode::Interactive, PermissionMode::PolicyBased, PermissionMode::Autonomous];
    for v in variants {
        let json = serde_json::to_string(&v).unwrap();
        let back: PermissionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
}

#[test]
fn permission_mode_default_is_policy_based() {
    assert_eq!(PermissionMode::default(), PermissionMode::PolicyBased);
}

#[test]
fn parse_config_with_shell_section() {
    let toml = r#"
[shell]
permission_mode = "autonomous"
auto_snapshot = false

[shell.rules]
"docker compose" = "safe"
"#;
    let config = WorkspaceConfig::from_toml(toml).unwrap();
    let shell = config.shell.unwrap();
    assert_eq!(shell.permission_mode, PermissionMode::Autonomous);
    assert_eq!(shell.auto_snapshot, Some(false));
    assert_eq!(shell.rules.unwrap().get("docker compose").unwrap(), "safe");
}
```

**Step 2: Write failing tests for RiskLevel**

In `backend/crates/sober-sandbox/src/risk.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_level_serde_roundtrip() {
        let variants = [RiskLevel::Safe, RiskLevel::Moderate, RiskLevel::Dangerous];
        for v in variants {
            let json = serde_json::to_string(&v).unwrap();
            let back: RiskLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(v, back);
        }
    }

    #[test]
    fn risk_level_ordering() {
        assert!(RiskLevel::Safe < RiskLevel::Moderate);
        assert!(RiskLevel::Moderate < RiskLevel::Dangerous);
    }
}
```

**Step 3: Implement PermissionMode**

Add to `workspace_config.rs` (extending the existing config from plan 017):

```rust
use std::collections::HashMap;

/// How much autonomy the agent has when executing shell commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Interactive,
    PolicyBased,
    Autonomous,
}

impl Default for PermissionMode {
    fn default() -> Self {
        Self::PolicyBased
    }
}

/// Sandbox execution policy for this workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSandboxConfig {
    #[serde(rename = "profile")]
    pub permission_profile: Option<String>,
    pub max_execution_seconds: Option<u32>,
    pub network_mode: Option<String>,
    pub allowed_domains: Option<Vec<String>>,
}

/// Shell execution settings for agent commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceShellConfig {
    pub permission_mode: PermissionMode,
    pub auto_snapshot: Option<bool>,
    pub max_snapshots: Option<u32>,
    pub rules: Option<HashMap<String, String>>,
}
```

Add `sandbox` and `shell` fields to `WorkspaceConfig`:

```rust
pub struct WorkspaceConfig {
    pub llm: Option<WorkspaceLlmConfig>,
    pub style: Option<WorkspaceStyleConfig>,
    pub sandbox: Option<WorkspaceSandboxConfig>,
    pub shell: Option<WorkspaceShellConfig>,
}
```

**Step 4: Implement RiskLevel**

In `backend/crates/sober-sandbox/src/risk.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Risk classification for shell commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Safe,
    Moderate,
    Dangerous,
}
```

Add `pub mod risk; pub use risk::RiskLevel;` to sober-sandbox's `lib.rs`.

**Step 5: Update workspace config template**

In `backend/crates/sober-workspace/src/fs.rs`, update `DEFAULT_CONFIG_TOML` to
include the new `[sandbox]` and `[shell]` sections (commented out, like the
existing `[llm]` and `[style]` sections):

```rust
const DEFAULT_CONFIG_TOML: &str = "\
# Workspace configuration for Sober agent.
# Uncomment and modify settings as needed.

# [llm]
# model = \"anthropic/claude-sonnet-4\"
# context_budget = 4096

# [style]
# tone = \"neutral\"
# commit_convention = \"conventional\"

# [sandbox]
# profile = \"standard\"            # locked_down | standard | unrestricted
# max_execution_seconds = 300
# network_mode = \"none\"           # none | allowed_domains | full
# allowed_domains = []

# [shell]
# permission_mode = \"policy_based\"  # interactive | policy_based | autonomous
# auto_snapshot = true
# max_snapshots = 10                 # oldest pruned when exceeded
#
# [shell.rules]
# \"docker compose\" = \"safe\"
# \"npm publish\" = \"dangerous\"
";
```

Also update the re-export in `sober-core/src/lib.rs` to include the new types:

```rust
pub use workspace_config::{
    WorkspaceConfig, WorkspaceAgentState,
    PermissionMode, WorkspaceSandboxConfig, WorkspaceShellConfig,
};
```

**Step 6: Run tests to verify they pass**

Run: `cargo test -p sober-core -q -- permission_mode && cargo test -p sober-sandbox -q -- risk_level`
Expected: PASS

**Step 7: Commit**

```bash
git add backend/crates/sober-core/src/workspace_config.rs backend/crates/sober-core/src/lib.rs backend/crates/sober-sandbox/src/risk.rs backend/crates/sober-sandbox/src/lib.rs backend/crates/sober-workspace/src/fs.rs
git commit -m "feat(core,sandbox): add PermissionMode, RiskLevel, and workspace config sections for shell execution"
```

---

## Task 2: Add CommandPolicy to sober-sandbox

**Files:**
- Create: `backend/crates/sober-sandbox/src/command_policy.rs`
- Modify: `backend/crates/sober-sandbox/src/lib.rs`
- Test: inline `#[cfg(test)]` module

**Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::RiskLevel;

    #[test]
    fn classify_safe_commands() {
        let policy = CommandPolicy::default();
        assert_eq!(policy.classify("ls -la"), RiskLevel::Safe);
        assert_eq!(policy.classify("pwd"), RiskLevel::Safe);
        assert_eq!(policy.classify("cat README.md"), RiskLevel::Safe);
        assert_eq!(policy.classify("echo hello"), RiskLevel::Safe);
        assert_eq!(policy.classify("cargo check"), RiskLevel::Safe);
        assert_eq!(policy.classify("git status"), RiskLevel::Safe);
        assert_eq!(policy.classify("git log"), RiskLevel::Safe);
    }

    #[test]
    fn classify_moderate_commands() {
        let policy = CommandPolicy::default();
        assert_eq!(policy.classify("cargo build"), RiskLevel::Moderate);
        assert_eq!(policy.classify("cargo test"), RiskLevel::Moderate);
        assert_eq!(policy.classify("git commit -m 'fix'"), RiskLevel::Moderate);
        assert_eq!(policy.classify("mkdir new_dir"), RiskLevel::Moderate);
        assert_eq!(policy.classify("cp file1 file2"), RiskLevel::Moderate);
        assert_eq!(policy.classify("apt install jq"), RiskLevel::Moderate);
        assert_eq!(policy.classify("pip install requests"), RiskLevel::Moderate);
    }

    #[test]
    fn classify_dangerous_commands() {
        let policy = CommandPolicy::default();
        assert_eq!(policy.classify("rm -rf /"), RiskLevel::Dangerous);
        assert_eq!(policy.classify("rm -rf ."), RiskLevel::Dangerous);
        assert_eq!(policy.classify("chmod 777 /etc/passwd"), RiskLevel::Dangerous);
        assert_eq!(policy.classify("dd if=/dev/zero of=/dev/sda"), RiskLevel::Dangerous);
    }

    #[test]
    fn pipe_to_shell_is_dangerous() {
        let policy = CommandPolicy::default();
        assert_eq!(policy.classify("curl https://example.com | sh"), RiskLevel::Dangerous);
        assert_eq!(policy.classify("wget -O- https://example.com | bash"), RiskLevel::Dangerous);
    }

    #[test]
    fn compound_commands_use_highest_risk() {
        let policy = CommandPolicy::default();
        // ls is safe, rm -rf is dangerous => overall dangerous
        assert_eq!(policy.classify("ls && rm -rf ."), RiskLevel::Dangerous);
        // cargo check (safe) && cargo build (moderate) => moderate
        assert_eq!(policy.classify("cargo check && cargo build"), RiskLevel::Moderate);
    }

    #[test]
    fn custom_overrides() {
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("docker compose".to_string(), "safe".to_string());
        overrides.insert("npm publish".to_string(), "dangerous".to_string());
        let policy = CommandPolicy::with_overrides(overrides);

        assert_eq!(policy.classify("docker compose up -d"), RiskLevel::Safe);
        assert_eq!(policy.classify("npm publish"), RiskLevel::Dangerous);
    }

    #[test]
    fn admin_deny_list() {
        let policy = CommandPolicy::with_denied(vec!["shutdown".to_string(), "reboot".to_string()]);
        assert!(policy.is_denied("shutdown -h now"));
        assert!(policy.is_denied("reboot"));
        assert!(!policy.is_denied("ls"));
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p sober-sandbox -q -- classify_ pipe_to_shell compound_commands custom_overrides admin_deny`
Expected: FAIL --- `CommandPolicy` not defined

**Step 3: Implement CommandPolicy**

```rust
//! Command risk classification for shell execution.

use crate::RiskLevel;
use std::collections::HashMap;

/// Classifies shell commands into risk tiers based on pattern matching.
pub struct CommandPolicy {
    overrides: HashMap<String, RiskLevel>,
    denied: Vec<String>,
}

impl Default for CommandPolicy {
    fn default() -> Self {
        Self {
            overrides: HashMap::new(),
            denied: Vec::new(),
        }
    }
}

impl CommandPolicy {
    /// Create a policy with user-defined command risk overrides.
    pub fn with_overrides(overrides: HashMap<String, String>) -> Self {
        let parsed = overrides
            .into_iter()
            .filter_map(|(cmd, level)| {
                let risk = match level.as_str() {
                    "safe" => RiskLevel::Safe,
                    "moderate" => RiskLevel::Moderate,
                    "dangerous" => RiskLevel::Dangerous,
                    _ => return None,
                };
                Some((cmd, risk))
            })
            .collect();
        Self {
            overrides: parsed,
            denied: Vec::new(),
        }
    }

    /// Create a policy with admin-level denied commands.
    pub fn with_denied(denied: Vec<String>) -> Self {
        Self {
            overrides: HashMap::new(),
            denied,
        }
    }

    /// Create a policy with both overrides and denied commands.
    pub fn new(overrides: HashMap<String, String>, denied: Vec<String>) -> Self {
        let mut policy = Self::with_overrides(overrides);
        policy.denied = denied;
        policy
    }

    /// Check if a command is on the admin deny list.
    pub fn is_denied(&self, command: &str) -> bool {
        let cmd = command.trim();
        self.denied.iter().any(|d| {
            cmd == d.as_str() || cmd.starts_with(&format!("{d} "))
        })
    }

    /// Classify a command's risk level.
    ///
    /// For compound commands (pipes, `&&`, `||`, `;`), returns the highest
    /// risk of any sub-command.
    pub fn classify(&self, command: &str) -> RiskLevel {
        // Split compound commands
        let sub_commands = split_compound(command);
        let mut highest = RiskLevel::Safe;

        // Check for pipe-to-shell pattern (entire command)
        if is_pipe_to_shell(command) {
            return RiskLevel::Dangerous;
        }

        for sub in &sub_commands {
            let sub = sub.trim();
            if sub.is_empty() {
                continue;
            }

            // Check user overrides first
            if let Some(risk) = self.check_override(sub) {
                if risk > highest {
                    highest = risk;
                }
                continue;
            }

            let risk = classify_single(sub);
            if risk > highest {
                highest = risk;
            }
        }

        highest
    }

    fn check_override(&self, command: &str) -> Option<RiskLevel> {
        // Try matching against override patterns (longest match wins)
        let mut best: Option<(&str, RiskLevel)> = None;
        for (pattern, risk) in &self.overrides {
            if command == pattern.as_str() || command.starts_with(&format!("{pattern} ")) {
                match best {
                    Some((prev, _)) if pattern.len() > prev.len() => {
                        best = Some((pattern, *risk));
                    }
                    None => {
                        best = Some((pattern, *risk));
                    }
                    _ => {}
                }
            }
        }
        best.map(|(_, risk)| risk)
    }
}

/// Split a command string on `&&`, `||`, `;` (but NOT pipes --- those are
/// handled separately by `is_pipe_to_shell`).
fn split_compound(command: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let bytes = command.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b';' {
            parts.push(&command[start..i]);
            start = i + 1;
        } else if i + 1 < bytes.len()
            && ((bytes[i] == b'&' && bytes[i + 1] == b'&')
                || (bytes[i] == b'|' && bytes[i + 1] == b'|'))
        {
            parts.push(&command[start..i]);
            start = i + 2;
            i += 1;
        }
        i += 1;
    }
    parts.push(&command[start..]);
    parts
}

/// Check if the command pipes into a shell interpreter.
fn is_pipe_to_shell(command: &str) -> bool {
    let shells = ["sh", "bash", "zsh", "fish", "dash"];
    if let Some(pipe_pos) = command.rfind('|') {
        let after_pipe = command[pipe_pos + 1..].trim();
        let first_word = after_pipe.split_whitespace().next().unwrap_or("");
        // Check bare shell name or path ending in shell
        shells.iter().any(|s| {
            first_word == *s || first_word.ends_with(&format!("/{s}"))
        })
    } else {
        false
    }
}

/// Classify a single (non-compound) command by its first word and flags.
fn classify_single(command: &str) -> RiskLevel {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return RiskLevel::Safe;
    }

    // Extract base command name (strip path prefix)
    let cmd = parts[0].rsplit('/').next().unwrap_or(parts[0]);
    let args_str = &parts[1..];

    match cmd {
        // Safe: read-only, informational
        "ls" | "cat" | "head" | "tail" | "less" | "more" | "pwd" | "echo"
        | "printf" | "which" | "whereis" | "whoami" | "id" | "env"
        | "printenv" | "date" | "wc" | "diff" | "file" | "stat"
        | "du" | "df" | "free" | "uptime" | "uname" | "hostname"
        | "find" | "locate" | "tree" | "grep" | "rg" | "ag"
        | "sort" | "uniq" | "tr" | "cut" | "awk" | "sed" | "jq" | "yq"
        | "man" | "help" | "type" | "true" | "false" | "test" => RiskLevel::Safe,

        // Safe: version/check commands
        "cargo" if args_str.first().is_some_and(|a| matches!(*a, "check" | "clippy" | "fmt" | "doc" | "search" | "tree" | "metadata")) => RiskLevel::Safe,
        "git" if args_str.first().is_some_and(|a| matches!(*a, "status" | "log" | "diff" | "show" | "branch" | "tag" | "stash" | "remote" | "ls-files" | "blame")) => RiskLevel::Safe,
        "node" if args_str.first().is_some_and(|a| *a == "--version" || *a == "-v") => RiskLevel::Safe,
        "python" | "python3" if args_str.first().is_some_and(|a| *a == "--version" || *a == "-V") => RiskLevel::Safe,
        "npm" if args_str.first().is_some_and(|a| matches!(*a, "ls" | "list" | "info" | "view" | "outdated" | "audit")) => RiskLevel::Safe,
        "pnpm" if args_str.first().is_some_and(|a| matches!(*a, "ls" | "list" | "info" | "outdated" | "audit")) => RiskLevel::Safe,

        // Dangerous: destructive or dangerous
        "rm" if has_recursive_force(args_str) => RiskLevel::Dangerous,
        "chmod" | "chown" | "chgrp" => RiskLevel::Dangerous,
        "dd" | "mkfs" | "fdisk" | "parted" | "mount" | "umount" => RiskLevel::Dangerous,
        "shutdown" | "reboot" | "poweroff" | "halt" | "init" => RiskLevel::Dangerous,
        "kill" | "killall" | "pkill" => RiskLevel::Dangerous,
        "iptables" | "ip6tables" | "nft" | "ufw" => RiskLevel::Dangerous,
        "su" | "sudo" => RiskLevel::Dangerous,
        "eval" | "exec" => RiskLevel::Dangerous,

        // Moderate: builds, installs, writes
        "cargo" => RiskLevel::Moderate,  // build, test, run, install, etc.
        "git" => RiskLevel::Moderate,    // commit, push, checkout, etc.
        "npm" | "pnpm" | "yarn" | "bun" => RiskLevel::Moderate,
        "pip" | "pip3" | "pipx" => RiskLevel::Moderate,
        "apt" | "apt-get" | "dnf" | "yum" | "pacman" | "brew" => RiskLevel::Moderate,
        "make" | "cmake" | "ninja" | "meson" => RiskLevel::Moderate,
        "docker" | "podman" => RiskLevel::Moderate,
        "mv" | "cp" | "mkdir" | "touch" | "rm" => RiskLevel::Moderate,
        "tee" | "patch" => RiskLevel::Moderate,
        "wget" | "curl" => RiskLevel::Moderate,
        "python" | "python3" | "node" | "ruby" | "perl" => RiskLevel::Moderate,
        "rustup" => RiskLevel::Moderate,

        // Default: moderate (unknown commands are not auto-safe)
        _ => RiskLevel::Moderate,
    }
}

/// Check if rm args include both -r and -f (in any combination).
fn has_recursive_force(args: &[&str]) -> bool {
    let mut has_r = false;
    let mut has_f = false;
    for arg in args {
        if arg.starts_with('-') && !arg.starts_with("--") {
            if arg.contains('r') || arg.contains('R') {
                has_r = true;
            }
            if arg.contains('f') {
                has_f = true;
            }
        }
    }
    has_r && has_f
}
```

**Step 4: Add property-based tests (security requirement)**

Per CLAUDE.md, security-critical code requires property-based testing. The command
classifier is security-sensitive — bypasses could let dangerous commands auto-approve.

Add `proptest` as a dev-dependency in `backend/crates/sober-sandbox/Cargo.toml`:

```toml
[dev-dependencies]
proptest = "1"
```

Add property-based tests to the test module:

```rust
    // --- Property-based tests (CLAUDE.md security requirement) ---

    use proptest::prelude::*;

    proptest! {
        /// Any command containing "rm -rf /" should be classified as Dangerous.
        #[test]
        fn rm_rf_root_always_dangerous(prefix in "[a-z ]{0,20}") {
            let policy = CommandPolicy::default();
            let cmd = format!("{prefix}; rm -rf /");
            prop_assert_eq!(policy.classify(&cmd), RiskLevel::Dangerous);
        }

        /// Pipe to shell is always dangerous regardless of the command before the pipe.
        #[test]
        fn pipe_to_shell_always_dangerous(cmd in "[a-zA-Z0-9 /._-]{1,50}") {
            let policy = CommandPolicy::default();
            let piped = format!("{cmd} | sh");
            prop_assert_eq!(policy.classify(&piped), RiskLevel::Dangerous);
        }

        /// Safe commands should never be classified as Dangerous.
        #[test]
        fn safe_commands_never_dangerous(args in "[a-zA-Z0-9 ./_-]{0,30}") {
            let policy = CommandPolicy::default();
            for safe_cmd in ["ls", "cat", "pwd", "echo", "whoami"] {
                let cmd = format!("{safe_cmd} {args}");
                prop_assert_ne!(policy.classify(&cmd), RiskLevel::Dangerous);
            }
        }

        /// Admin deny list should always deny, regardless of arguments.
        #[test]
        fn deny_list_always_denies(args in "[a-zA-Z0-9 ./_-]{0,30}") {
            let policy = CommandPolicy::with_denied(vec!["shutdown".to_string()]);
            let cmd = format!("shutdown {args}");
            prop_assert!(policy.is_denied(&cmd));
        }
    }
```

> **Known limitations (document in code):**
> The classifier uses naive string splitting and does NOT handle:
> - Quoted strings (`echo "rm -rf /"` would be mis-classified)
> - Command substitution (`$(rm -rf .)`, backticks)
> - Escape sequences (`r\m -rf /`)
> - Heredocs, process substitution
>
> The sandbox (bwrap) is the real security boundary. The classifier is a
> UX convenience for the permission system, not a security gate. Add a doc
> comment to `CommandPolicy::classify()` documenting these limitations.

**Step 5: Verify sober-sandbox has sober-core dependency**

In `backend/crates/sober-sandbox/Cargo.toml`, ensure:

```toml
sober-core = { path = "../sober-core" }
```

(May already exist from plan 009. If not, add it.)

Add to `lib.rs`:

```rust
pub mod command_policy;
pub mod risk;
pub use command_policy::CommandPolicy;
pub use risk::RiskLevel;
```

**Step 6: Run all tests to verify they pass**

Run: `cargo test -p sober-sandbox -q`
Expected: PASS (including proptest)

**Step 7: Run clippy**

Run: `cargo clippy -p sober-sandbox -q -- -D warnings`
Expected: PASS

**Step 8: Commit**

```bash
git add backend/crates/sober-sandbox/
git commit -m "feat(sandbox): add CommandPolicy with property-based security tests"
```

---

## Task 3: Add Confirmation Proto Messages

**Files:**
- Modify: `backend/proto/sober/agent/v1/agent.proto`

**Step 1: Add new messages to agent.proto**

Add these message types:

```protobuf
message ConfirmRequest {
  string confirm_id = 1;
  string command = 2;
  string risk_level = 3;
  repeated string affects = 4;
  string reason = 5;
}

message ConfirmResponse {
  string confirm_id = 1;
  bool approved = 2;
}
```

Add `confirm_request` to the `AgentEvent.event` oneof:

```protobuf
message AgentEvent {
  oneof event {
    TextDelta text_delta = 1;
    ToolCallStart tool_call_start = 2;
    ToolCallResult tool_call_result = 3;
    Done done = 4;
    Error error = 5;
    ThinkingDelta thinking_delta = 6;
    TitleGenerated title_generated = 7;
    ConfirmRequest confirm_request = 8;
  }
}
```

**Step 2: Add SubmitConfirmation RPC to AgentService**

The API gateway needs a way to route `ConfirmResponse` messages back to the
agent process. Add a new RPC method to `AgentService`:

```protobuf
service AgentService {
  // ... existing RPCs ...

  /// Route a user's confirmation response to the agent's ConfirmationBroker.
  /// Called by sober-api when it receives a chat.confirm_response WebSocket message.
  rpc SubmitConfirmation(ConfirmResponse) returns (google.protobuf.Empty);
}
```

This closes the loop: agent emits `ConfirmRequest` via the streaming `AgentEvent`,
the API gateway translates it to a `chat.confirm` WebSocket message, the user
responds in the UI, the frontend sends `chat.confirm_response` over WebSocket,
and the API gateway calls `SubmitConfirmation` RPC to deliver the response back
to the agent's `ConfirmationBroker`.

**Step 3: Verify proto compiles**

Run: `cargo build -p sober-agent -q`
Expected: PASS (tonic-build generates code from proto)

**Step 4: Commit**

```bash
git add backend/proto/
git commit -m "feat(proto): add ConfirmRequest, ConfirmResponse, and SubmitConfirmation RPC"
```

---

## Task 4: Add Confirmation Channel to Agent

**Files:**
- Modify: `backend/crates/sober-agent/src/agent.rs`
- Create: `backend/crates/sober-agent/src/confirm.rs`
- Modify: `backend/crates/sober-agent/src/lib.rs`

This task adds the mechanism for the agent loop to pause on a tool call and
wait for user confirmation.

**Step 1: Write failing test for confirmation channel**

In `backend/crates/sober-agent/src/confirm.rs`:

```rust
//! Confirmation channel for interactive shell command approval.

use tokio::sync::{mpsc, oneshot};

/// A request for the user to confirm a command.
#[derive(Debug)]
pub struct ConfirmRequest {
    pub confirm_id: String,
    pub command: String,
    pub risk_level: String,
    pub affects: Vec<String>,
    pub reason: String,
    pub response_tx: oneshot::Sender<bool>,
}

/// Handle for sending confirmation responses back to the agent.
#[derive(Debug, Clone)]
pub struct ConfirmationSender {
    tx: mpsc::Sender<ConfirmResponse>,
}

/// Response from the user.
#[derive(Debug)]
pub struct ConfirmResponse {
    pub confirm_id: String,
    pub approved: bool,
}

/// Broker that matches confirmation responses to pending requests.
pub struct ConfirmationBroker {
    pending: std::collections::HashMap<String, oneshot::Sender<bool>>,
    rx: mpsc::Receiver<ConfirmResponse>,
}

impl ConfirmationBroker {
    /// Create a new broker and its sender handle.
    pub fn new() -> (Self, ConfirmationSender) {
        let (tx, rx) = mpsc::channel(32);
        let broker = Self {
            pending: std::collections::HashMap::new(),
            rx,
        };
        let sender = ConfirmationSender { tx };
        (broker, sender)
    }

    /// Register a pending confirmation. Returns a oneshot receiver that
    /// resolves when the user responds.
    pub fn register(&mut self, confirm_id: String) -> oneshot::Receiver<bool> {
        let (tx, rx) = oneshot::channel();
        self.pending.insert(confirm_id, tx);
        rx
    }

    /// Process one incoming response. Call this in a select loop.
    pub async fn process_next(&mut self) -> Option<()> {
        let resp = self.rx.recv().await?;
        if let Some(tx) = self.pending.remove(&resp.confirm_id) {
            let _ = tx.send(resp.approved);
        }
        Some(())
    }
}

impl ConfirmationSender {
    /// Send a confirmation response.
    pub async fn respond(&self, confirm_id: String, approved: bool) -> Result<(), mpsc::error::SendError<ConfirmResponse>> {
        self.tx.send(ConfirmResponse { confirm_id, approved }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn confirmation_roundtrip() {
        let (mut broker, sender) = ConfirmationBroker::new();
        let rx = broker.register("test-1".to_string());

        // Simulate user approving
        tokio::spawn(async move {
            sender.respond("test-1".to_string(), true).await.unwrap();
        });

        broker.process_next().await.unwrap();
        let approved = rx.await.unwrap();
        assert!(approved);
    }

    #[tokio::test]
    async fn confirmation_deny() {
        let (mut broker, sender) = ConfirmationBroker::new();
        let rx = broker.register("test-2".to_string());

        tokio::spawn(async move {
            sender.respond("test-2".to_string(), false).await.unwrap();
        });

        broker.process_next().await.unwrap();
        let approved = rx.await.unwrap();
        assert!(!approved);
    }

    #[tokio::test]
    async fn unknown_confirm_id_ignored() {
        let (mut broker, sender) = ConfirmationBroker::new();
        let _rx = broker.register("known".to_string());

        tokio::spawn(async move {
            sender.respond("unknown".to_string(), true).await.unwrap();
        });

        broker.process_next().await.unwrap();
        // known request is still pending (not resolved by unknown response)
    }
}
```

**Step 2: Run tests to verify they pass**

Run: `cargo test -p sober-agent -q -- confirmation_`
Expected: PASS

**Step 3: Add module to lib.rs**

```rust
pub mod confirm;
```

**Step 4: Commit**

```bash
git add backend/crates/sober-agent/src/confirm.rs backend/crates/sober-agent/src/lib.rs
git commit -m "feat(agent): add confirmation broker for interactive command approval"
```

---

## Task 5: Implement ShellTool

**Files:**
- Create: `backend/crates/sober-agent/src/tools/shell.rs`
- Modify: `backend/crates/sober-agent/src/tools/mod.rs`
- Test: inline `#[cfg(test)]` module

**Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shell_tool_metadata() {
        let tool = ShellTool::new_for_test();
        let meta = tool.metadata();
        assert_eq!(meta.name, "shell");
        assert!(meta.context_modifying);
        // Verify schema has required "command" field
        let schema = &meta.input_schema;
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("command")));
    }

    #[test]
    fn shell_tool_rejects_missing_command() {
        let tool = ShellTool::new_for_test();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute(json!({})));
        assert!(result.is_err());
    }

    #[test]
    fn shell_tool_rejects_empty_command() {
        let tool = ShellTool::new_for_test();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute(json!({"command": ""})));
        assert!(result.is_err());
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p sober-agent -q -- shell_tool_`
Expected: FAIL --- `ShellTool` not defined

**Step 3: Ensure SandboxPolicy derives Clone**

The `ShellTool::execute_inner` method calls `self.sandbox_policy.clone()` to build
a per-execution policy. If `SandboxPolicy` doesn't already derive `Clone` (from plan
009), add it now:

```rust
// In sober-sandbox/src/lib.rs (or wherever SandboxPolicy is defined)
#[derive(Debug, Clone)]
pub struct SandboxPolicy { ... }
```

**Step 4: Implement ShellTool**

Create `backend/crates/sober-agent/src/tools/shell.rs`:

```rust
//! Shell command execution tool for the agent.
//!
//! Executes commands in a sandboxed environment (bwrap) within the user's
//! workspace. Supports permission modes and confirmation flow for sensitive
//! commands.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use sober_core::types::tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
use sober_core::PermissionMode;
use sober_sandbox::RiskLevel;
use sober_sandbox::{BwrapSandbox, CommandPolicy, SandboxPolicy};
use tokio::sync::oneshot;

/// Maximum output length returned to the LLM to avoid blowing up context.
const MAX_OUTPUT_LEN: usize = 16_000;

/// Default per-command timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u32 = 300;

#[derive(Debug, Deserialize)]
struct ShellInput {
    command: String,
    workdir: Option<String>,
    timeout: Option<u32>,
}

/// Callback type for requesting user confirmation.
/// Returns a oneshot receiver that resolves to the user's decision.
pub type ConfirmFn = Arc<
    dyn Fn(ConfirmPayload) -> oneshot::Receiver<bool> + Send + Sync,
>;

/// Payload sent to the frontend for confirmation.
#[derive(Debug, Clone)]
pub struct ConfirmPayload {
    pub confirm_id: String,
    pub command: String,
    pub risk_level: RiskLevel,
    pub affects: Vec<String>,
    pub reason: String,
}

/// Shell command execution tool.
pub struct ShellTool {
    policy: CommandPolicy,
    permission_mode: PermissionMode,
    workspace_home: PathBuf,
    sandbox_policy: SandboxPolicy,
    confirm_fn: Option<ConfirmFn>,
    auto_snapshot: bool,
}

impl ShellTool {
    /// Create a new ShellTool.
    pub fn new(
        policy: CommandPolicy,
        permission_mode: PermissionMode,
        workspace_home: PathBuf,
        sandbox_policy: SandboxPolicy,
        confirm_fn: Option<ConfirmFn>,
        auto_snapshot: bool,
    ) -> Self {
        Self {
            policy,
            permission_mode,
            workspace_home,
            sandbox_policy,
            confirm_fn,
            auto_snapshot,
        }
    }

    #[cfg(test)]
    fn new_for_test() -> Self {
        Self {
            policy: CommandPolicy::default(),
            permission_mode: PermissionMode::Autonomous,
            workspace_home: PathBuf::from("/tmp/test-workspace"),
            sandbox_policy: SandboxPolicy::from(sober_sandbox::SandboxProfile::LockedDown),
            confirm_fn: None,
            auto_snapshot: false,
        }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let input: ShellInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("invalid input: {e}")))?;

        if input.command.trim().is_empty() {
            return Err(ToolError::InvalidInput("command cannot be empty".into()));
        }

        // Check admin deny list
        if self.policy.is_denied(&input.command) {
            return Ok(ToolOutput {
                content: "Command denied by system policy.".to_string(),
                is_error: true,
            });
        }

        // Classify risk
        let risk = self.policy.classify(&input.command);

        // Check permission mode
        let needs_confirmation = match (self.permission_mode, risk) {
            (PermissionMode::Autonomous, _) => false,
            (PermissionMode::PolicyBased, RiskLevel::Safe | RiskLevel::Moderate) => false,
            (PermissionMode::Interactive, _) | (PermissionMode::PolicyBased, RiskLevel::Dangerous) => true,
            _ => true,
        };

        if needs_confirmation {
            if let Some(ref confirm_fn) = self.confirm_fn {
                let payload = ConfirmPayload {
                    confirm_id: uuid::Uuid::now_v7().to_string(),
                    command: input.command.clone(),
                    risk_level: risk,
                    affects: Vec::new(), // TODO: analyze affected files
                    reason: format!("Command classified as {risk:?}"),
                };
                let rx = (confirm_fn)(payload);
                match tokio::time::timeout(
                    std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS as u64),
                    rx,
                ).await {
                    Ok(Ok(true)) => {} // approved
                    Ok(Ok(false)) => {
                        return Ok(ToolOutput {
                            content: "Command denied by user.".to_string(),
                            is_error: false,
                        });
                    }
                    Ok(Err(_)) => {
                        return Ok(ToolOutput {
                            content: "Confirmation cancelled.".to_string(),
                            is_error: true,
                        });
                    }
                    Err(_) => {
                        return Ok(ToolOutput {
                            content: "Command timed out waiting for confirmation.".to_string(),
                            is_error: true,
                        });
                    }
                }
            } else {
                // No confirmation handler available, deny by default
                return Ok(ToolOutput {
                    content: format!("Command requires confirmation but no confirmation handler is available. Risk level: {risk:?}"),
                    is_error: true,
                });
            }
        }

        // TODO: When auto_snapshot is enabled and risk == Dangerous,
        // create a workspace snapshot before execution. Use
        // sober_workspace::SnapshotManager::create() (implemented in plan 017,
        // Task 11). Snapshot dir is at `.sober/snapshots/` within the workspace.
        // After creating, call SnapshotManager::prune(max_snapshots) to enforce
        // the workspace's configured retention limit.

        // Determine working directory
        let workdir = if let Some(ref wd) = input.workdir {
            self.workspace_home.join(wd)
        } else {
            self.workspace_home.clone()
        };

        // Build sandbox with workspace bind-mount
        let mut policy = self.sandbox_policy.clone();
        policy.fs_read.push(self.workspace_home.clone());
        policy.fs_write.push(self.workspace_home.clone());
        // System tool paths for read-only access
        for sys_path in ["/usr", "/bin", "/lib", "/lib64", "/etc/alternatives"] {
            let p = PathBuf::from(sys_path);
            if p.exists() && !policy.fs_read.contains(&p) {
                policy.fs_read.push(p);
            }
        }

        let timeout = input.timeout.unwrap_or(DEFAULT_TIMEOUT_SECS);
        policy.max_execution_seconds = timeout;

        let sandbox = BwrapSandbox::new(policy);

        // Execute via shell -c to support pipes, redirects, etc.
        let command = vec![
            "sh".to_string(),
            "-c".to_string(),
            format!("cd {} && {}", workdir.display(), input.command),
        ];

        let result = sandbox
            .execute(&command, &HashMap::new())
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("sandbox error: {e}")))?;

        // Format output
        let mut output = String::new();
        output.push_str(&format!("Exit code: {}\n", result.exit_code));

        if !result.stdout.is_empty() {
            output.push_str("\nstdout:\n");
            output.push_str(&result.stdout);
        }

        if !result.stderr.is_empty() {
            output.push_str("\nstderr:\n");
            output.push_str(&result.stderr);
        }

        // Truncate if too long
        if output.len() > MAX_OUTPUT_LEN {
            output.truncate(MAX_OUTPUT_LEN);
            output.push_str("\n\n[output truncated]");
        }

        Ok(ToolOutput {
            content: output,
            is_error: result.exit_code != 0,
        })
    }
}

impl Tool for ShellTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "shell".to_string(),
            description: "Execute a shell command in the user's workspace. Use for building, \
                testing, file operations, installing tools, and running scripts. Commands run \
                in a sandboxed environment with the user's workspace mounted."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "workdir": {
                        "type": "string",
                        "description": "Working directory relative to workspace root (optional)"
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in seconds (optional, defaults to 300)"
                    }
                },
                "required": ["command"]
            }),
            context_modifying: true,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}
```

**Step 4: Register ShellTool in mod.rs**

Add to `backend/crates/sober-agent/src/tools/mod.rs`:

```rust
pub mod shell;
pub use shell::ShellTool;
```

**Step 5: Run tests to verify they pass**

Run: `cargo test -p sober-agent -q -- shell_tool_`
Expected: PASS

**Step 6: Run clippy**

Run: `cargo clippy -p sober-agent -q -- -D warnings`
Expected: PASS

**Step 7: Commit**

```bash
git add backend/crates/sober-agent/src/tools/
git commit -m "feat(agent): add ShellTool for sandboxed command execution"
```

---

## Task 6: Wire Confirmation Flow Through WebSocket

**Files:**
- Modify: `backend/crates/sober-api/src/routes/ws.rs`
- Modify: `frontend/src/lib/types/index.ts`

**Step 1: Add new WebSocket message types**

In `ws.rs`, add handling for the new proto event `confirm_request`:

```rust
Some(proto::agent_event::Event::ConfirmRequest(cr)) => {
    ServerWsMessage::ChatConfirm {
        conversation_id,
        confirm_id: cr.confirm_id,
        command: cr.command,
        risk_level: cr.risk_level,
        affects: cr.affects,
        reason: cr.reason,
    }
}
```

Add handling for the incoming `chat.confirm_response` client message. The API
gateway calls the agent's `SubmitConfirmation` gRPC RPC (added in Task 3) to
route the response back to the agent's `ConfirmationBroker`:

```rust
"chat.confirm_response" => {
    let confirm_id = msg["confirm_id"].as_str().unwrap_or("").to_string();
    let approved = msg["approved"].as_bool().unwrap_or(false);
    // Route to the agent via SubmitConfirmation gRPC RPC
    let resp = proto::ConfirmResponse { confirm_id, approved };
    if let Err(e) = agent_client.submit_confirmation(resp).await {
        tracing::warn!("failed to submit confirmation: {e}");
    }
}
```

**Step 2: Add TypeScript types**

In `frontend/src/lib/types/index.ts`, add:

```typescript
export interface ConfirmRequest {
    confirm_id: string;
    command: string;
    risk_level: 'safe' | 'moderate' | 'dangerous';
    affects: string[];
    reason: string;
}

// Add to ServerWsMessage union:
| { type: 'chat.confirm'; conversation_id: string; confirm_id: string; command: string; risk_level: string; affects: string[]; reason: string }
```

**Step 3: Verify it compiles**

Run: `cargo build -p sober-api -q && cd frontend && pnpm check`
Expected: PASS

**Step 4: Commit**

```bash
git add backend/crates/sober-api/src/routes/ws.rs frontend/src/lib/types/
git commit -m "feat(api): wire confirmation flow through WebSocket"
```

---

## Task 7: Frontend ConfirmationCard Component

**Files:**
- Create: `frontend/src/lib/components/chat/ConfirmationCard.svelte`
- Modify: `frontend/src/routes/(app)/chat/[id]/+page.svelte`
- Modify: `frontend/src/lib/stores/websocket.svelte.ts`

**Step 1: Create ConfirmationCard component**

`frontend/src/lib/components/chat/ConfirmationCard.svelte`:

```svelte
<script lang="ts">
  import type { ConfirmRequest } from '$lib/types';

  interface Props {
    request: ConfirmRequest;
    resolved?: 'approved' | 'denied';
    onRespond: (confirmId: string, approved: boolean) => void;
  }

  let { request, resolved, onRespond }: Props = $props();

  const riskColors: Record<string, string> = {
    safe: 'bg-emerald-500/20 text-emerald-400 border-emerald-500/30',
    moderate: 'bg-amber-500/20 text-amber-400 border-amber-500/30',
    dangerous: 'bg-red-500/20 text-red-400 border-red-500/30',
  };

  const riskBadge = $derived(riskColors[request.risk_level] ?? riskColors.moderate);
</script>

<div class="rounded-lg border border-zinc-700 bg-zinc-800/50 p-4 my-2">
  <div class="flex items-center gap-2 mb-3">
    <span class="text-sm font-medium text-zinc-300">Command Approval</span>
    <span class="px-2 py-0.5 rounded text-xs font-medium border {riskBadge}">
      {request.risk_level}
    </span>
  </div>

  <pre class="bg-zinc-900 rounded px-3 py-2 text-sm font-mono text-zinc-200 overflow-x-auto mb-2">{request.command}</pre>

  {#if request.affects.length > 0}
    <div class="text-xs text-zinc-400 mb-2">
      <span class="font-medium">Affects:</span>
      {#each request.affects as item}
        <span class="ml-1">{item}</span>
      {/each}
    </div>
  {/if}

  {#if request.reason}
    <p class="text-xs text-zinc-500 mb-3">{request.reason}</p>
  {/if}

  {#if resolved}
    <div class="text-sm font-medium {resolved === 'approved' ? 'text-emerald-400' : 'text-red-400'}">
      {resolved === 'approved' ? 'Approved' : 'Denied'}
    </div>
  {:else}
    <div class="flex gap-2">
      <button
        onclick={() => onRespond(request.confirm_id, true)}
        class="px-3 py-1.5 rounded text-sm font-medium bg-emerald-600 hover:bg-emerald-500 text-white transition-colors"
      >
        Approve
      </button>
      <button
        onclick={() => onRespond(request.confirm_id, false)}
        class="px-3 py-1.5 rounded text-sm font-medium bg-zinc-600 hover:bg-zinc-500 text-zinc-200 transition-colors"
      >
        Deny
      </button>
    </div>
  {/if}
</div>
```

**Step 2: Handle confirm events in chat page**

In `+page.svelte`, add handling for `chat.confirm` in `handleWsMessage`:

```typescript
case 'chat.confirm': {
    // Store pending confirmation for display
    pendingConfirms = [...pendingConfirms, {
        confirm_id: msg.confirm_id,
        command: msg.command,
        risk_level: msg.risk_level as 'safe' | 'moderate' | 'dangerous',
        affects: msg.affects,
        reason: msg.reason,
    }];
    break;
}
```

Add a respond handler:

```typescript
function handleConfirmResponse(confirmId: string, approved: boolean) {
    ws.send({
        type: 'chat.confirm_response',
        conversation_id: conversationId,
        confirm_id: confirmId,
        approved,
    });
    // Mark as resolved in UI
    const idx = pendingConfirms.findIndex(c => c.confirm_id === confirmId);
    if (idx >= 0) {
        resolvedConfirms[confirmId] = approved ? 'approved' : 'denied';
    }
}
```

Render `ConfirmationCard` in the message flow alongside tool calls.

**Step 3: Verify frontend compiles**

Run: `cd frontend && pnpm check`
Expected: PASS

**Step 4: Commit**

```bash
git add frontend/src/lib/components/chat/ConfirmationCard.svelte frontend/src/routes/\(app\)/chat/\[id\]/+page.svelte frontend/src/lib/stores/websocket.svelte.ts
git commit -m "feat(frontend): add ConfirmationCard for shell command approval"
```

---

## Task 8: Permission Mode Status Bar

**Files:**
- Create: `frontend/src/lib/components/chat/StatusBar.svelte`
- Modify: `frontend/src/routes/(app)/chat/[id]/+page.svelte`

**Step 1: Create StatusBar component**

`frontend/src/lib/components/chat/StatusBar.svelte`:

```svelte
<script lang="ts">
  interface Props {
    mode: 'interactive' | 'policy_based' | 'autonomous';
    onModeChange: (mode: 'interactive' | 'policy_based' | 'autonomous') => void;
  }

  let { mode, onModeChange }: Props = $props();

  const modes = [
    { value: 'interactive' as const, label: 'Interactive', color: 'emerald' },
    { value: 'policy_based' as const, label: 'Policy', color: 'amber' },
    { value: 'autonomous' as const, label: 'Autonomous', color: 'red' },
  ] as const;

  const modeOrder: Array<'interactive' | 'policy_based' | 'autonomous'> = ['interactive', 'policy_based', 'autonomous'];

  function cycleMode() {
    const idx = modeOrder.indexOf(mode);
    const next = modeOrder[(idx + 1) % modeOrder.length];
    onModeChange(next);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.ctrlKey && e.shiftKey && e.key === 'P') {
      e.preventDefault();
      cycleMode();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex items-center justify-between px-3 py-1.5 bg-zinc-900/50 border-t border-zinc-800 text-xs">
  <div class="flex items-center gap-1 rounded-md bg-zinc-800 p-0.5">
    {#each modes as m}
      <button
        onclick={() => onModeChange(m.value)}
        class="px-2 py-1 rounded transition-colors {mode === m.value
          ? m.color === 'emerald' ? 'bg-emerald-600/30 text-emerald-400'
          : m.color === 'amber' ? 'bg-amber-600/30 text-amber-400'
          : 'bg-red-600/30 text-red-400'
          : 'text-zinc-500 hover:text-zinc-300'}"
      >
        {m.label}
      </button>
    {/each}
  </div>
  <span class="text-zinc-600">Ctrl+Shift+P</span>
</div>
```

**Step 2: Add StatusBar to chat page**

Place below the chat input area in `+page.svelte`. Wire `mode` state
to workspace settings (load on mount, persist on change via API).

**Step 3: Verify frontend compiles**

Run: `cd frontend && pnpm check`
Expected: PASS

**Step 4: Commit**

```bash
git add frontend/src/lib/components/chat/StatusBar.svelte frontend/src/routes/\(app\)/chat/\[id\]/+page.svelte
git commit -m "feat(frontend): add permission mode status bar with Ctrl+Shift+P toggle"
```

---

## Task 9: Wire ShellTool into Agent Startup

**Files:**
- Modify: `backend/crates/sober-agent/src/main.rs`
- Modify: `backend/crates/sober-agent/src/agent.rs`

**Step 1: Register ShellTool in agent startup**

In `main.rs`, create the ShellTool with workspace config and add it to the
tool registry alongside web_search and fetch_url:

```rust
use sober_agent::tools::ShellTool;
use sober_sandbox::CommandPolicy;
use sober_core::PermissionMode;

// Create shell tool with workspace-derived config
let command_policy = CommandPolicy::default();
let shell_tool = ShellTool::new(
    command_policy,
    PermissionMode::PolicyBased, // default, overridden per-conversation
    workspace_home,
    sandbox_policy,
    Some(confirm_fn),
    true, // auto_snapshot
);

let builtins: Vec<Arc<dyn Tool>> = vec![
    Arc::new(web_search_tool),
    Arc::new(fetch_url_tool),
    Arc::new(shell_tool),
];
let registry = ToolRegistry::with_builtins(builtins);
```

**Step 2: Wire confirmation broker into agent gRPC service**

The `AgentGrpcService` needs access to the `ConfirmationSender` so it can
route incoming `ConfirmResponse` messages to the broker. Add the sender as
a field on the gRPC service and implement the `SubmitConfirmation` RPC:

```rust
#[tonic::async_trait]
impl AgentService for AgentGrpcService {
    // ... existing RPCs ...

    async fn submit_confirmation(
        &self,
        request: tonic::Request<proto::ConfirmResponse>,
    ) -> Result<tonic::Response<()>, tonic::Status> {
        let resp = request.into_inner();
        self.confirmation_sender
            .respond(resp.confirm_id, resp.approved)
            .await
            .map_err(|_| tonic::Status::internal("confirmation broker unavailable"))?;
        Ok(tonic::Response::new(()))
    }
}
```

**Step 3: Verify it compiles**

Run: `cargo build -p sober-agent -q`
Expected: PASS

**Step 4: Commit**

```bash
git add backend/crates/sober-agent/src/
git commit -m "feat(agent): wire ShellTool and confirmation broker into agent startup"
```

---

## Task 10: Add Workspace Permission Mode API Endpoint

**Files:**
- Modify: `backend/crates/sober-api/src/routes/` (add workspace settings route)

**Step 1: Add PUT endpoint for workspace settings**

```rust
/// Update workspace shell permission mode.
async fn update_workspace_settings(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<UpdateWorkspaceSettings>,
) -> Result<impl IntoResponse, AppError> {
    // Read workspace config.toml
    // Update permission_mode field
    // Write back
    // Return updated settings
}

#[derive(Debug, Deserialize)]
struct UpdateWorkspaceSettings {
    permission_mode: Option<String>,
    auto_snapshot: Option<bool>,
}
```

**Step 2: Register route**

Add to the workspace router: `PUT /api/v1/workspaces/:id/settings`

**Step 3: Verify it compiles**

Run: `cargo build -p sober-api -q`
Expected: PASS

**Step 4: Commit**

```bash
git add backend/crates/sober-api/src/routes/
git commit -m "feat(api): add workspace settings endpoint for permission mode"
```

---

## Task 11: Integration Test --- End-to-End Shell Execution

**Files:**
- Create: `backend/crates/sober-agent/tests/shell_integration.rs`

**Step 1: Write integration test**

```rust
//! Integration test for shell tool execution.
//! Requires bwrap to be installed.

#[tokio::test]
async fn shell_tool_executes_basic_command() {
    // Create a temp workspace directory
    // Create ShellTool with Autonomous mode (no confirmation needed)
    // Execute "echo hello"
    // Assert output contains "hello"
    // Assert exit_code is 0
}

#[tokio::test]
async fn shell_tool_captures_stderr() {
    // Execute "ls /nonexistent"
    // Assert stderr contains error message
    // Assert exit_code is non-zero
}

#[tokio::test]
async fn shell_tool_respects_workdir() {
    // Create subdirectory in temp workspace
    // Execute "pwd" with workdir set to subdirectory
    // Assert output contains subdirectory path
}

#[tokio::test]
async fn shell_tool_denies_blocked_commands() {
    // Create policy with "shutdown" in deny list
    // Execute "shutdown -h now"
    // Assert output says denied by system policy
}
```

**Step 2: Run integration tests**

Run: `cargo test -p sober-agent -q --test shell_integration`
Expected: PASS (requires bwrap installed)

**Step 3: Commit**

```bash
git add backend/crates/sober-agent/tests/shell_integration.rs
git commit -m "test(agent): add integration tests for shell tool execution"
```

---

## Task 12: Final Verification & Version Bump

**Files:**
- Modify: All affected `Cargo.toml` versions (MINOR bump for feature)
- Modify: `frontend/package.json` version

**Step 1: Run full test suite**

```bash
cargo test --workspace -q
cd frontend && pnpm check && pnpm test --silent
```

Expected: All tests pass.

**Step 2: Run clippy on all crates**

```bash
cargo clippy --workspace -q -- -D warnings
```

Expected: No warnings.

**Step 3: MINOR version bump (feature PR)**

Bump version in ALL affected crate `Cargo.toml` files. This is a `feat/` branch, so
the version bump MUST be MINOR (`0.X.0`), NEVER patch.

**Step 4: Move plan to done**

```bash
git mv docs/plans/active/022-shell-execution docs/plans/done/022-shell-execution
```

**Step 5: Commit**

```bash
git add .
git commit -m "chore: version bump and move plan 022 to done"
```
