# 043: Workspace Capability Filtering — Plan

## Step 1: Migration — add columns to `workspace_settings`

**Files:** `backend/migrations/`

1. Create migration adding `disabled_tools TEXT[] NOT NULL DEFAULT '{}'` and
   `disabled_plugins UUID[] NOT NULL DEFAULT '{}'` to `workspace_settings`.
2. No data migration needed — all workspaces start with empty lists (everything
   enabled).
3. Run `cargo sqlx prepare` to update offline query data.

## Step 2: Domain types — extend `WorkspaceSettings`

**Files:** `backend/crates/sober-core/src/types/`

1. Add `disabled_tools: Vec<String>` and `disabled_plugins: Vec<PluginId>` to
   the `WorkspaceSettings` struct.
2. Update serde derives if needed (Vec serializes naturally to JSON arrays).

**Tests:**
- Unit test: `WorkspaceSettings` serde round-trip with non-empty disabled lists.

## Step 3: DB layer — update `PgWorkspaceSettingsRepo`

**Files:** `backend/crates/sober-db/src/repos/workspace_settings.rs`

1. Update `get()` SELECT to include `disabled_tools`, `disabled_plugins`.
2. Update `upsert()` INSERT/UPDATE to include new columns.
3. Update row type and `From<Row>` conversion.

**Tests:**
- Integration test: upsert settings with disabled tools/plugins, read back,
  verify values.

## Step 4: Enforcement — filter tools per turn

**Files:**
- `backend/crates/sober-plugin/src/manager.rs`
- `backend/crates/sober-agent/src/tools/bootstrap.rs`

1. **Plugin filter:** In `PluginManager::tools_for_turn()`, after querying
   enabled plugins, filter out any whose ID is in
   `workspace_settings.disabled_plugins`.
2. **Tool name filter:** In `ToolBootstrap::build()`, after assembling the full
   tool list (built-in + plugin tools), filter out any whose name is in
   `workspace_settings.disabled_tools`.
3. Both read `disabled_*` from `TurnContext.workspace_settings` (added by 042).

**Tests:**
- Unit test: `ToolBootstrap` with disabled_tools excludes the named tools.
- Unit test: `PluginManager` with disabled_plugins skips those plugins.

## Step 5: gRPC — `ListTools` RPC

**Files:**
- `backend/proto/agent.proto`
- `backend/crates/sober-agent/src/grpc/`

1. Add `ListTools` RPC definition to `agent.proto`.
2. Implement handler: collect built-in tool names from `ToolBootstrap` static
   list + plugin-exported tools from `PluginManager`. Return unfiltered catalog
   with source attribution.
3. The response includes ALL tools regardless of disabled lists — the frontend
   uses this to show toggles.

## Step 6: API — `GET /tools` endpoint

**Files:** `backend/crates/sober-api/src/routes/`

1. Add `GET /api/v1/tools` route that proxies `ListTools` gRPC.
2. Update workspace settings endpoints (from 042) to include `disabled_tools`
   and `disabled_plugins` in GET response and PATCH input.

**Tests:**
- Integration test: `GET /tools` returns built-in and plugin tools.
- Integration test: `PATCH /conversations/{id}/settings` with `disabled_tools`,
  read back via GET, verify persistence.

## Step 7: Frontend — capabilities settings section

**Files:**
- `frontend/src/lib/services/` — new tools service
- `frontend/src/lib/components/` — capabilities section component
- `frontend/src/routes/(app)/chat/` — integrate into settings panel

1. Create `tools.ts` service: `listTools()` calling `GET /api/v1/tools`.
2. Create capabilities settings component with:
   - Plugin toggles (from existing `GET /plugins`)
   - Tool toggles (from `GET /tools`)
   - Free-text input for power users
3. Wire toggles to `PATCH /conversations/{id}/settings` with updated
   `disabled_tools` / `disabled_plugins` arrays.
4. Integrate into workspace settings panel from 042.

## Verification

1. Disable `shell` via PATCH, send message asking agent to run a command —
   agent should not have shell.
2. Re-enable shell, verify it works again.
3. Disable a plugin by UUID, verify its tools vanish from `GET /tools`.
4. Frontend: toggle a tool off, verify persistence and agent respects it.
5. `cargo test --workspace -q`
6. `cargo clippy -q -- -D warnings`
7. `pnpm check && pnpm test --silent`
