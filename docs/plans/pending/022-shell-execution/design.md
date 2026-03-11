# 022 --- Agent Shell Execution

**Date:** 2026-03-11
**Depends on:** 017 (workspaces & artifacts)

---

## Overview

Give the agent the ability to execute shell commands in a user's server-side
workspace. Commands run inside the existing bwrap sandbox (`sober-sandbox`),
with a configurable permission system that lets users choose how much autonomy
the agent has. A confirmation flow in the chat UI allows users to approve or
deny sensitive commands before execution.

---

## 1. Core Concepts

### ShellTool

A new tool in the agent's tool registry, alongside `web_search` and
`fetch_url`. The LLM invokes it like any other tool:

```json
{
  "name": "shell",
  "parameters": {
    "command": "cargo build --release",
    "workdir": "projects/my-app",
    "timeout": 120
  }
}
```

The tool classifies the command's risk level, checks the user's permission
mode, optionally requests confirmation, then delegates to `BwrapSandbox` for
sandboxed execution.

### Permission Modes

Three modes, user-selectable per workspace:

| Mode | Behavior |
|------|----------|
| **Interactive** | Every command requires explicit user approval |
| **PolicyBased** (default) | Safe/moderate commands auto-approve; dangerous commands require approval |
| **Autonomous** | All commands auto-approve within sandbox constraints |

Users can switch modes at any time via:
- A segmented control in the chat UI status bar
- Keyboard shortcut (`Ctrl+Shift+P` to cycle)
- Chat command ("set permission mode to autonomous")
- API endpoint (`PUT /api/v1/workspaces/{id}/settings`)
- CLI (`soberctl workspace configure --mode autonomous`)

The selected mode persists in workspace settings (`.sober/config.toml`)
and survives page reloads and session restarts.

### Command Risk Classification

Commands are classified into three tiers:

| Risk | Examples | PolicyBased behavior |
|------|----------|---------------------|
| **Safe** | `ls`, `cat`, `pwd`, `echo`, `cargo check`, `git status` | Auto-approve |
| **Moderate** | `cargo build`, `apt install`, `git commit`, `mkdir`, `cp` | Auto-approve |
| **Dangerous** | `rm -rf`, `curl \| sh`, `chmod 777`, `dd`, `mkfs` | Require approval |

Classification uses static pattern matching:
- Parse command into sub-commands (split on `|`, `;`, `&&`, `||`)
- Match each sub-command against configurable pattern rules
- Overall risk = highest risk of any sub-command
- Pipe to shell (`| sh`, `| bash`) escalates any command to Dangerous

### Admin-Level Deny List

System administrators can define a hard deny list via configuration
(`BLOCKED_COMMANDS` env var or config file). These commands are rejected
regardless of user permission mode --- users cannot override them.

---

## 2. Confirmation Flow

When a command requires user approval, the agent pauses and sends a
confirmation request through the WebSocket.

### Protocol

New proto messages:

```proto
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

New WebSocket events:

```
Server → Client: chat.confirm { conversation_id, confirm_id, command, risk_level, affects, reason }
Client → Server: chat.confirm_response { conversation_id, confirm_id, approved }
```

### Agent Behavior

1. ShellTool emits `ConfirmRequest` via the agent event stream
2. API gateway translates to `chat.confirm` WebSocket message
3. Agent loop suspends the tool call, waiting on a oneshot channel
4. User approves or denies in the UI
5. Client sends `chat.confirm_response` over WebSocket
6. API gateway calls `SubmitConfirmation` gRPC RPC on the agent service
7. Agent's `ConfirmationBroker` resolves the oneshot channel
8. Agent resumes: executes command (approved) or returns denial (denied)

### Timeout

If the user doesn't respond within a configurable timeout (default: 5
minutes), the command is auto-denied. This prevents the agent loop from
hanging indefinitely.

---

## 3. Frontend UI

### Confirmation Card (`ConfirmationCard.svelte`)

Rendered inline in the chat when a `chat.confirm` event arrives:

- Command text in monospace, syntax-highlighted
- Risk level badge: green (Safe), yellow (Moderate), red (Dangerous)
- "Affects" list showing files/directories the command touches
- Reason text explaining why the command was flagged
- **Approve** and **Deny** buttons
- After decision: card updates to show resolved state ("Approved" / "Denied")
- Disabled state after resolution (no double-clicks)

### Permission Mode Status Bar

A thin status bar below the chat input area:

- Segmented control with three states: Interactive | PolicyBased | Autonomous
- Current mode visually highlighted with color coding
- `Ctrl+Shift+P` keyboard shortcut cycles through modes
- Mode change persisted to workspace settings via API call
- On page load, reads workspace settings and reflects current mode
- Extensible: future status indicators can share this bar

---

## 4. Workspace Integration

### Sandbox Configuration

The ShellTool constructs a `SandboxPolicy` from workspace settings:

- `fs_read`: workspace home directory (read access)
- `fs_write`: workspace home directory (write access)
- `fs_deny`: sensitive paths (`.ssh`, `.aws`, `.gnupg` --- already handled by sober-sandbox)
- `network_mode`: from workspace `config.toml` (None / AllowedDomains / Full)
- `max_execution_seconds`: from workspace settings (default 300)
- Additional read-only binds for system tools (`/usr`, `/bin`, etc.)

### Snapshots

When `auto_snapshot` is enabled (default: `true`) and a Dangerous command is
approved, the workspace manager creates a snapshot before execution. Users can
disable auto-snapshots in workspace settings.

Snapshot management uses whatever mechanism plan 017 provides (likely
filesystem copy or tar for v1).

### Tool Installation

The agent can install packages within its sandbox. Installed tools persist in
the workspace's `home/.local/` directory across conversations. The sandbox
bind-mounts this directory so tools are available in subsequent executions.

---

## 5. Configuration Knobs

### Per-Workspace (user-controlled, in `.sober/config.toml`)

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `permission_mode` | enum | `policy_based` | `interactive` / `policy_based` / `autonomous` |
| `auto_snapshot` | bool | `true` | Auto-snapshot before dangerous commands |
| `max_snapshots` | u32 | `10` | Maximum snapshots retained per workspace (oldest pruned first) |
| `max_execution_seconds` | u32 | `300` | Per-command timeout |
| `network_mode` | enum | `none` | `none` / `allowed_domains` / `full` |
| `allowed_domains` | list | `[]` | Domains allowed when network_mode = allowed_domains |

### Per-Workspace Command Rules (user overrides)

```toml
[shell.rules]
"docker compose" = "safe"
"npm publish" = "dangerous"
```

Users can promote or demote commands within the range the admin allows.

### System-Level (admin-controlled, env vars / config)

| Setting | Default | Description |
|---------|---------|-------------|
| `WORKSPACE_ROOT` | `/var/lib/sober/workspaces/` | Base path for all workspaces |
| `MAX_WORKSPACE_SIZE` | (unset) | Disk quota per workspace |
| `DEFAULT_PERMISSION_MODE` | `policy_based` | System-wide default |
| `BLOCKED_COMMANDS` | (empty) | Admin deny list, cannot be overridden |
| `CONFIRM_TIMEOUT_SECONDS` | `300` | Auto-deny timeout for confirmations |

---

## 6. Crate Placement

| Component | Crate | Rationale |
|-----------|-------|-----------|
| `ShellTool` | `sober-agent` | Tool in the agent's registry, alongside web_search |
| `CommandPolicy` | `sober-sandbox` | Risk classification is a sandbox concern |
| `PermissionMode` enum | `sober-core` | Shared type used by workspace, agent, and API |
| `RiskLevel` enum | `sober-sandbox` | Shell/sandbox concern, not needed in base crate |
| Confirm proto messages | `backend/proto/` | New messages in agent.proto |
| Confirm WebSocket events | `sober-api` | New WS event types in ws.rs |
| `ConfirmationCard.svelte` | `frontend` | New chat component |
| Permission status bar | `frontend` | New layout component |

---

## 7. Security Considerations

- All commands execute inside bwrap sandbox --- process isolation, filesystem
  restrictions, optional network isolation
- Sensitive paths always denied (`.ssh`, `.aws`, `.gnupg`) regardless of config
- Admin deny list is a hard floor --- users cannot override
- Pipe-to-shell patterns (`curl | sh`) always classified as Dangerous
- Command audit logging via existing `SandboxAuditEntry` mechanism
- Confirmation timeout prevents indefinite agent suspension
- Workspace isolation: each user's workspace is bind-mounted separately,
  no cross-user filesystem access

---

## 8. Impact on Existing Code

### Modified crates

- **sober-agent** --- new `ShellTool` in tools module, confirmation channel in
  agent loop
- **sober-sandbox** --- new `CommandPolicy` for risk classification
- **sober-core** --- new `PermissionMode` and `RiskLevel` enums
- **sober-api** --- new WebSocket event types (`chat.confirm`,
  `chat.confirm_response`), confirmation routing

### New frontend components

- `ConfirmationCard.svelte` --- inline command approval card
- `PermissionToggle.svelte` --- status bar permission mode control

### Proto changes

- New `ConfirmRequest` and `ConfirmResponse` messages in `agent.proto`
- New `confirm` variant in `AgentEvent` oneof

---

## 9. Out of Scope

- Direct terminal access (WebSocket PTY) --- future enhancement
- User's local machine access --- not planned
- Multi-command batch approval --- v1 does one command at a time
- Resource limits (cgroups) --- future enhancement, sandbox handles basic
  timeout enforcement for now
