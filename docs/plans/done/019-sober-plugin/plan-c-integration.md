# #019 Plan C: Integration & Self-Evolution — COMPLETED

**Goal:** Wire the unified plugin system into the agent, API, and frontend. Migrate MCP servers to the plugins table. Add gRPC RPCs for plugin management. Build sober-plugin-gen for skill and WASM generation. Deliver the `/generate-plugin` tool. Auto-sync filesystem skills into the database.

**Architecture:** `PluginManager` coordinates MCP pools, skill loading, and WASM hosts — replacing direct usage of `McpPool` and `SkillLoader` in `ToolBootstrap`. API routes proxy plugin operations to the agent via gRPC. The frontend has a unified `/settings/plugins` page. `sober-plugin-gen` provides LLM-powered plugin generation with a self-correcting compile-test loop.

**Tech Stack:** Rust, tonic/prost (gRPC), SvelteKit, Tailwind CSS, sober-llm.

---

## Implemented (all tasks complete)

### Backend — Core

- [x] **MCP data migration** — `20260321000001_migrate_mcp_to_plugins.sql` moves `mcp_servers` rows into `plugins` table, drops `mcp_servers`
- [x] **Legacy MCP types removed** — `McpServerRepo`, `McpServerId`, `McpServerConfig`, `CreateMcpServer`, `UpdateMcpServer` all removed from `sober-core` and `sober-db`
- [x] **PluginManager** (`sober-plugin/src/manager.rs`) — wraps `McpPool`, `SkillLoader`, WASM `PluginHost` cache. Provides `tools_for_turn()` that collects tools from all three plugin kinds
- [x] **ToolBootstrap rewired** — `sober-agent/src/tools/bootstrap.rs` uses `PluginManager` instead of direct `McpPool` + `SkillLoader`
- [x] **AgentRepos extended** — `type Plg: PluginRepo` + `fn plugins()` added to the `AgentRepos` trait

### Backend — Skill auto-sync

- [x] **Filesystem → DB sync** — `PluginManager::skill_tools()` auto-registers newly-discovered filesystem skills into the `plugins` table with name, description, path, scope, and `workspace_id`
- [x] **Startup sync** — agent boot calls `tools_for_turn()` to register user-level skills (`~/.sober/skills/`) immediately, before any ListSkills call
- [x] **Workspace awareness** — workspace-scoped skills are synced with the correct `workspace_id`. Skills are keyed by `(name, workspace_id)` to avoid cross-workspace deduplication
- [x] **Disabled skills excluded** — skills marked disabled in the DB are filtered from both the slash command palette and the `ActivateSkillTool` catalog
- [x] **Stale cleanup job** — `system::skill_plugin_cleanup` scheduler job (every 1h) removes plugin entries whose filesystem paths no longer exist

### Backend — gRPC RPCs

- [x] **8 plugin RPCs** added to `AgentService`: `ListPlugins`, `InstallPlugin`, `UninstallPlugin`, `EnablePlugin`, `DisablePlugin`, `ImportPlugins`, `ReloadPlugins`, `ChangePluginScope`
- [x] **ListSkills** — queries DB for enabled skill plugins, filters by scope (system + user always; workspace only if matching current conversation's workspace)
- [x] **ReloadSkills** — invalidates skill cache, re-syncs filesystem → DB with workspace context, returns filtered results
- [x] **ChangePluginScope** — moves skill files between `~/.sober/skills/` and `<workspace>/.sober/skills/`, updates DB scope + config path

### Backend — Plugin generation

- [x] **sober-plugin-gen crate** — `scaffold.rs` (template scaffolding), `compile.rs` (WASM compilation via cargo), `generate.rs` (LLM-powered generation with retry loop)
- [x] **GeneratePluginTool** — agent tool (`/generate-plugin`) that generates skills or WASM plugins from natural language descriptions, saves files, registers in DB

### Backend — API routes

- [x] **Unified `/api/v1/plugins`** — 8 endpoints: list, install, import, get, update (enable/disable/config/scope), delete, audit, reload
- [x] **Skill routes preserved** — `/api/v1/skills` and `/api/v1/skills/reload` kept under the plugins module for slash command palette compatibility
- [x] **Old routes removed** — `mcp.rs` and `skills.rs` deleted from `sober-api`
- [x] **`PluginRepo::update_scope`** — new repo method for scope changes

### Frontend

- [x] **`/settings/plugins` page** — filter tabs (All/MCP/Skills/WASM), plugin list with inline badges
- [x] **Plugin row layout** — name → version → scope dropdown → kind badge → status badge
- [x] **Scope dropdown** — inline select with workspace option only shown for workspace-scoped plugins (no way to move back to unknown workspace)
- [x] **Actions** — enable/disable toggle, audit log viewer, delete with confirmation
- [x] **Header actions** — Add MCP, Import Config, Reload buttons in header row
- [x] **MCP install form** — name, command, args, env, description
- [x] **Config import** — paste `.mcp.json` content for batch import
- [x] **Old pages removed** — `settings/mcp/` page and `services/mcp.ts` deleted
- [x] **Plugin types** — `Plugin`, `PluginKind`, `PluginStatus`, `PluginScope` TypeScript types with `scope` field

### Agent instructions

- [x] **Slash command instructions** — `tools.md` instruction file teaches the LLM to activate skills via `activate_skill` tool when user sends `/skill-name` messages
- [x] **Tool description** — `activate_skill` tool description explains slash command behavior

---

## File structure (final)

### New files

| File | Responsibility |
|------|---------------|
| `backend/crates/sober-plugin/src/manager.rs` | `PluginManager` — wraps McpPool + SkillLoader + WASM hosts, skill auto-sync |
| `backend/crates/sober-plugin-gen/` | Generation crate: `error.rs`, `scaffold.rs`, `compile.rs`, `generate.rs` |
| `backend/crates/sober-agent/src/tools/generate_plugin.rs` | `GeneratePluginTool` — `/generate-plugin` agent tool |
| `backend/crates/sober-api/src/routes/plugins.rs` | Unified plugin + skill API routes |
| `backend/crates/sober-scheduler/src/executors/skill_plugin_cleanup.rs` | Stale skill plugin cleanup executor |
| `backend/migrations/20260321000001_migrate_mcp_to_plugins.sql` | MCP → plugins data migration |
| `frontend/src/routes/(app)/settings/plugins/+page.svelte` | Plugins management page |
| `frontend/src/lib/services/plugins.ts` | Plugin API client |
| `frontend/src/lib/types/plugin.ts` | Plugin TypeScript types |

### Deleted files

| File | Reason |
|------|--------|
| `backend/crates/sober-db/src/repos/mcp_servers.rs` | Replaced by `PgPluginRepo` |
| `backend/crates/sober-api/src/routes/mcp.rs` | Replaced by `plugins.rs` |
| `backend/crates/sober-api/src/routes/skills.rs` | Merged into `plugins.rs` |
| `frontend/src/routes/(app)/settings/mcp/+page.svelte` | Replaced by plugins page |
| `frontend/src/lib/services/mcp.ts` | Replaced by `plugins.ts` |

---

## Known follow-ups

- [ ] **Plugin permissions** — scope changes, audit, delete should require admin role; users can only delete own workspace-scoped plugins
- [ ] **Consistent config keys** — `generate_plugin` tool uses `skill_path` while auto-sync uses `path`; needs alignment before cleanup job runs
- [ ] **Scope change file moves** — `ChangePluginScope` RPC moves files but workspace → user/system is one-way (no way to specify target workspace for the reverse)
