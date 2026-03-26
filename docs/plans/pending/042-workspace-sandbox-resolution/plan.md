# 042: Wire Workspace Sandbox Config Resolution — Plan

## Step 1: Type bridge — `WorkspaceSandboxConfig` → `SandboxConfig`

**Files:** `backend/crates/sober-core/src/workspace_config.rs`

Add `impl WorkspaceSandboxConfig` with a `to_sandbox_config()` method that
produces a `sober_sandbox::config::SandboxConfig`. Maps:
- `permission_profile` → `SandboxProfile`
- `network_mode` + `allowed_domains` → used in overrides
- `max_execution_seconds` → override

This avoids adding `sober-sandbox` as a dependency of `sober-core`. Instead,
return an intermediate struct or put the conversion in `sober-agent` where both
crates are available.

**Alternative:** Place the conversion in `sober-agent/src/tools/bootstrap.rs`
as a free function since that's the only call site.

## Step 2: Load workspace config in `ToolBootstrap::build()`

**Files:** `backend/crates/sober-agent/src/tools/bootstrap.rs`

In `ToolBootstrap::build()`, after resolving `shell_workspace`:

1. Attempt to read `{workspace_dir}/.sober/config.toml`.
2. Parse via `WorkspaceConfig::from_toml()`.
3. Convert `sandbox` section to `SandboxConfig`.
4. Call `resolve_policy(None, workspace_sandbox.as_ref(), None)`.
5. If resolution succeeds, use that policy. If it fails (parse error, missing
   file), log a warning and fall back to `self.shell.sandbox_policy`.

Pass the resolved policy into `ShellTool::new()` instead of always using the
startup policy.

## Step 3: Make `ShellTool::new` accept per-turn policy

**Files:** `backend/crates/sober-agent/src/tools/shell.rs`,
`backend/crates/sober-agent/src/tools/bootstrap.rs`

Currently `ShellTool::new()` reads `config.sandbox_policy`. Add an optional
`policy_override: Option<SandboxPolicy>` parameter (or just pass the resolved
policy directly). If provided, use it instead of the config default.

Keep `ShellToolConfig.sandbox_policy` as the fallback.

## Step 4: Tests

**Files:** `backend/crates/sober-agent/src/tools/bootstrap.rs` (unit tests)

- Test conversion of `WorkspaceSandboxConfig` with various fields.
- Test that `build()` with a workspace dir containing a config produces the
  expected policy.
- Test fallback when config file is missing or malformed.

## Step 5: Verify on server

Deploy updated binary to `sober.lan`, confirm the workspace config we wrote
earlier (`019d2990.../.sober/config.toml` with `profile = "unrestricted"`)
is picked up without changing the system-level config.
