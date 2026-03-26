# 042: Workspace Settings — Plan

## Step 1: Migration — `workspace_settings` table

**Files:** `backend/migrations/`

1. Create new migration with:
   - `sandbox_net_mode` enum (`none`, `allowed_domains`, `full`)
   - `workspace_settings` table (schema from design.md)
   - `sandbox_profile` is TEXT, not an enum
2. Data migration: for each workspace that has a conversation with a
   `permission_mode` value, insert a `workspace_settings` row carrying that value.
   Workspaces without conversations get default settings.
3. Drop `permission_mode` column from `conversations`.
4. Run `cargo sqlx prepare` to update offline query data.

## Step 2: Domain types — `WorkspaceSettings`

**Files:** `backend/crates/sober-core/src/types/`

1. Add `WorkspaceSettings` struct with all columns from the table.
   `sandbox_profile` is `String`, `sandbox_net_mode` is `Option<SandboxNetMode>`.
2. Add `SandboxNetMode` enum to `sober-core` types (DB-facing, distinct from
   `sober-sandbox::NetMode` which carries data).
3. Add `WorkspaceSettingsRepo` trait: `get(workspace_id)`, `upsert(settings)`.
4. Add conversion: `WorkspaceSettings` → `sober_sandbox::SandboxPolicy`
   (resolve profile defaults, apply non-null overrides). Place this in
   `sober-agent` where both crates are available.
5. Remove `permission_mode` from `Conversation` domain type and input types.
6. Delete `workspace_config.rs` entirely (`WorkspaceConfig`,
   `WorkspaceSandboxConfig`, `WorkspaceShellConfig`, `WorkspaceLlmConfig`,
   `WorkspaceStyleConfig`). Remove references in `sober-core/src/lib.rs`,
   `sober-workspace/src/fs.rs`, `sober-api/src/routes/workspaces.rs`.

**Tests:**
- Unit test: `SandboxNetMode` serde round-trip.

## Step 3: DB layer — `PgWorkspaceSettingsRepo` + `provision()`

**Files:** `backend/crates/sober-db/src/repos/`

1. Create `workspace_settings.rs` with `PgWorkspaceSettingsRepo`.
2. Implement `get` (by workspace_id) and `upsert` (INSERT ON CONFLICT UPDATE).
3. Add `WorkspaceRepo::provision(tx, user_id, name, root_path) -> (Workspace, WorkspaceSettings)`:
   - Takes caller's transaction — always part of a larger query chain.
   - Creates workspace row + workspace_settings row (defaults).
   - Shared logic callable from both API and agent.
4. Update conversation repo: remove `permission_mode` from SELECT/INSERT queries.
5. Update `CONV_COLUMNS` constant and `ConversationRow` to drop `permission_mode`.

**Tests:**
- Integration test: `provision()` creates both workspace + settings atomically.
- Integration test: `get` returns correct defaults after provision.
- Integration test: `upsert` updates fields, leaves others unchanged.

## Step 4: API layer — conversation creation + settings endpoint

**Files:** `backend/crates/sober-api/src/routes/conversations.rs`,
`backend/crates/sober-api/src/routes/workspaces.rs`

1. Update `POST /conversations`:
   - Remove `workspace_id` from request body.
   - Call `WorkspaceRepo::provision()` to create workspace + settings.
   - Create conversation with the new workspace_id.
   - All DB ops in one transaction.
2. Add `GET /conversations/{id}/settings`:
   - Load conversation (for `agent_mode`) + workspace_settings (for everything else).
   - Return flat combined response.
3. Add `PATCH /conversations/{id}/settings`:
   - All fields optional — partial update, omitted fields unchanged.
   - Supports quick toggles (single field) and full panel saves.
   - Update `conversations.agent_mode` + upsert `workspace_settings` in one tx.
4. Remove `permission_mode`, `workspace_id`, `agent_mode` from
   `PATCH /conversations/{id}`. Keep title + archived only.
5. Remove or deprecate file-based `GET/PUT /workspaces/{id}/settings`.

**Tests:**
- Integration test: `POST /conversations` returns conversation with workspace.
- Integration test: `GET /conversations/{id}/settings` returns defaults.
- Integration test: `PATCH /conversations/{id}/settings` partial update round-trip.
- Integration test: `PATCH` with single field only changes that field.

## Step 5: Agent — simplify `ensure_workspace()` + sandbox resolution

**Files:** `backend/crates/sober-agent/src/`

1. Simplify `ensure_workspace()`:
   - Normal path: workspace already exists (created by API). Resolve dir +
     load `WorkspaceSettings` from DB.
   - Fallback: if `workspace_id` is None, call `WorkspaceRepo::provision()`
     (same shared logic as API) + link to conversation.
2. Store settings in `TurnContext`
   (add `workspace_settings: Option<WorkspaceSettings>`).
3. In `ToolBootstrap::build()`:
   - If workspace settings exist, resolve `SandboxPolicy` from them.
   - Apply non-null overrides on top of profile defaults.
   - If no settings, fall back to `self.shell.sandbox_policy` (startup default).
4. Pass resolved policy into `ShellTool::new()`.

**Tests:**
- Unit test: `WorkspaceSettings` → `SandboxPolicy` conversion with override
  combinations (all null, some set, all set).
- Unit test: unknown profile name falls back to standard + warning.
- Unit test: `SandboxNetMode` + `allowed_domains` correctly combine into `NetMode`.

## Step 6: Frontend — settings panel update

**Files:** `frontend/src/lib/`

1. Add settings service methods: `getSettings(conversationId)`,
   `updateSettings(conversationId, Partial<ConversationSettings>)`.
2. Update `ConversationSettings.svelte`:
   - Fetch settings via `GET /conversations/{id}/settings` on open.
   - Re-arrange layout into grouped sections:
     - **Conversation**: agent_mode (group only)
     - **Workspace**: permission_mode, auto_snapshot, sandbox settings
   - Sandbox UI: profile selector (3 built-ins only), network mode selector,
     dynamic domain list (add/remove entries, shown only when net mode =
     `allowed_domains`), timeout input, allow spawn toggle.
   - Save via `PATCH /conversations/{id}/settings` — partial updates.
   - Remove `permissionMode` prop — loaded from settings endpoint.
3. Update TypeScript types:
   - Add `ConversationSettings` type matching the GET response.
   - Remove `permission_mode` from `Conversation` type.
4. Update any component that reads `conversation.permission_mode` to use
   the settings endpoint instead.

**Tests:**
- Update `ConversationSettings.test.ts` for new layout and settings flow.
- Test partial update sends only changed fields.

## Step 7: Docs & architecture update

**Files:** `ARCHITECTURE.md`, `docs/`

1. Update ARCHITECTURE.md: document `workspace_settings` table, update
   the crate map if workspace config scope changed.
2. Update any docs referencing `.sober/config.toml` sandbox/shell sections
   to reflect they now live in DB-backed workspace settings.
3. Document that `.sober/config.toml` is removed — all workspace settings
   are DB-backed via `workspace_settings` table.

## Step 8: Final verification

1. `cargo test --workspace -q`
2. `cargo clippy -q -- -D warnings`
3. `cargo fmt --check -q`
4. `pnpm check`
5. `pnpm test --silent`
