# 042: Wire Workspace Sandbox Config Resolution

## Problem

The agent resolves sandbox policy once at startup from the system config
(`/etc/sober/config.toml` → `agent.sandbox_profile`) and applies it to all
conversations. The existing `resolve_policy()` function in `sober-sandbox`
supports a layered resolution chain (tool → workspace → user → system default),
but the agent never calls it. Workspace `.sober/config.toml` files are ignored.

This means per-workspace sandbox customization (network access, timeouts,
custom profiles) has no effect.

## Root Cause

In `sober-agent/src/main.rs` (lines 116–123), the sandbox profile is parsed
from `config.agent.sandbox_profile` and resolved into a `SandboxPolicy` with
an empty custom profiles map. This policy is stored in `ShellToolConfig` and
reused for every conversation turn.

`ToolBootstrap::build()` receives a `TurnContext` with `workspace_dir` but
never reads `.sober/config.toml` from it.

## Fix

Load workspace config at tool-build time and pass it through the existing
`resolve_policy()` chain.

### Changes

1. **`ToolBootstrap::build()`** — when `workspace_dir` is set, read
   `.sober/config.toml` and extract the `[sandbox]` section. Convert
   `WorkspaceSandboxConfig` → `sober_sandbox::SandboxConfig`.

2. **`ShellTool` construction** — call `resolve_policy(None, workspace_sandbox,
   None)` instead of using the pre-resolved `self.shell.sandbox_policy`.
   Fall back to the startup policy if workspace config is absent or fails to
   parse.

3. **Type bridge** — add a `From<WorkspaceSandboxConfig>` (or conversion
   method) for `sober_sandbox::config::SandboxConfig` since the workspace
   config uses different field names (`permission_profile`, `network_mode`)
   than the sandbox crate's native config type.

4. **Caching** — workspace config files rarely change mid-conversation.
   Cache the parsed `WorkspaceConfig` in `TurnContext` or read it once per
   actor startup (in `ensure_workspace`) rather than on every turn. Invalidate
   on file mtime change.

### What stays the same

- System-level `sandbox_profile` in `/etc/sober/config.toml` remains the
  default when no workspace config exists.
- `ShellToolConfig.sandbox_policy` stays as the fallback policy.
- The `resolve_policy()` function and `SandboxConfig` types are unchanged.
- bwrap execution, audit logging, and command classification are unaffected.

## Scope

- `sober-agent`: `tools/bootstrap.rs`, `tools/shell.rs`, `conversation.rs`
- `sober-core`: `workspace_config.rs` (conversion method)
- `sober-sandbox`: no changes (existing code is correct)

## Testing

- Unit test: `WorkspaceSandboxConfig` → `SandboxConfig` conversion.
- Unit test: `ToolBootstrap::build()` with workspace config produces different
  policy than without.
- Integration test: workspace with `[sandbox] profile = "unrestricted"` results
  in `NetMode::Full` policy on the shell tool.
