# #019 Plan C: Integration & Self-Evolution

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the unified plugin system into the agent, API, and frontend. Migrate MCP servers to the plugins table. Add gRPC RPCs for plugin management. Build sober-plugin-gen for skill and WASM generation. Deliver the `/generate-plugin` slash command.

**Architecture:** `PluginManager` coordinates MCP pools, skill loading, and WASM hosts — replacing direct usage of `McpPool` and `SkillLoader` in `ToolBootstrap`. API routes proxy plugin operations to the agent via gRPC. The frontend gets a unified `/settings/plugins` page. `sober-plugin-gen` provides LLM-powered plugin generation with a self-correcting compile-test loop.

**Tech Stack:** Rust, tonic/prost (gRPC), SvelteKit, Tailwind CSS, sober-llm.

**Prerequisites:** Plan A (Registry) and Plan B (WASM Runtime) must be implemented.

**Design doc:** `docs/plans/pending/019-sober-plugin/design.md` — sections 9-14.

---

## File Structure

### New files

| File | Responsibility |
|------|---------------|
| `backend/crates/sober-plugin/src/manager.rs` | `PluginManager` — wraps McpPool + SkillLoader + WASM hosts |
| `backend/crates/sober-plugin-gen/Cargo.toml` | Generation crate manifest |
| `backend/crates/sober-plugin-gen/src/lib.rs` | Module declarations |
| `backend/crates/sober-plugin-gen/src/error.rs` | `GenError` enum |
| `backend/crates/sober-plugin-gen/src/scaffold.rs` | Template scaffolding (no LLM) |
| `backend/crates/sober-plugin-gen/src/generate.rs` | LLM-powered generation with retry loop |
| `backend/crates/sober-plugin-gen/src/compile.rs` | WASM compilation (shell out to cargo) |
| `backend/proto/sober/agent/v1/plugin.proto` | Plugin management gRPC messages |
| `backend/crates/sober-api/src/routes/plugins.rs` | Unified plugin API routes |
| `frontend/src/routes/(app)/settings/plugins/+page.svelte` | Plugins management page |
| `frontend/src/lib/services/plugins.ts` | Plugin API client |
| `frontend/src/lib/types/plugin.ts` | Plugin TypeScript types |

### Modified files

| File | Change |
|------|--------|
| `backend/crates/sober-plugin/Cargo.toml` | Add `sober-mcp`, `sober-skill` dependencies |
| `backend/crates/sober-plugin/src/lib.rs` | Add `manager` module |
| `backend/crates/sober-agent/src/tools/bootstrap.rs` | Replace `McpPool` + `SkillLoader` with `PluginManager` |
| `backend/crates/sober-agent/Cargo.toml` | Add `sober-plugin` dependency |
| `backend/proto/sober/agent/v1/agent.proto` | Add plugin management RPCs |
| `backend/crates/sober-api/src/routes/mod.rs` | Add plugin routes, remove MCP/Skill routes |
| `backend/crates/sober-core/src/types/repo.rs` | Remove `McpServerRepo` |
| `backend/crates/sober-core/src/types/ids.rs` | Remove `McpServerId` |
| `backend/crates/sober-core/src/types/domain.rs` | Remove `McpServerConfig` |
| `backend/crates/sober-core/src/types/input.rs` | Remove `CreateMcpServer`, `UpdateMcpServer` |
| `backend/crates/sober-db/src/repos/mcp_servers.rs` | Delete file |
| `frontend/src/routes/(app)/settings/mcp/+page.svelte` | Delete file (replaced by plugins page) |
| `frontend/src/lib/services/mcp.ts` | Delete file (replaced by plugins service) |

### Migration

| File | Purpose |
|------|---------|
| `backend/migrations/YYYYMMDD000001_migrate_mcp_to_plugins.sql` | Move `mcp_servers` data into `plugins`, drop `mcp_servers` |

---

## Task 1: MCP data migration

**Files:**
- Create: migration SQL file

- [ ] **Step 1:** Write migration that inserts `mcp_servers` rows into `plugins` table
- [ ] **Step 2:** Drop `mcp_servers` table in the same migration
- [ ] **Step 3:** Verify with `sqlx migrate info`
- [ ] **Step 4:** Commit

---

## Task 2: Remove McpServerRepo and related types

**Files:**
- Modify: `sober-core` (repo.rs, ids.rs, domain.rs, input.rs, agent_repos.rs)
- Delete: `sober-db/src/repos/mcp_servers.rs`

- [ ] **Step 1:** Remove `McpServerRepo` trait from `repo.rs`
- [ ] **Step 2:** Remove `McpServerId` from `ids.rs`
- [ ] **Step 3:** Remove `McpServerConfig` from `domain.rs`
- [ ] **Step 4:** Remove `CreateMcpServer`, `UpdateMcpServer` from `input.rs`
- [ ] **Step 5:** Remove `Mcp` associated type from `AgentRepos` trait
- [ ] **Step 6:** Delete `mcp_servers.rs` from sober-db, update `repos/mod.rs`
- [ ] **Step 7:** Fix all compilation errors in dependent crates
- [ ] **Step 8:** Run `cargo build -q`, commit

---

## Task 3: Implement PluginManager

**Files:**
- Create: `backend/crates/sober-plugin/src/manager.rs`
- Modify: `backend/crates/sober-plugin/Cargo.toml` (add sober-mcp, sober-skill deps)

`PluginManager` wraps `McpPool`, `SkillLoader`, and WASM hosts behind
one interface. Provides `tools_for_turn()` that returns all tools from
enabled plugins.

- [ ] **Step 1:** Add `sober-mcp` and `sober-skill` dependencies to Cargo.toml
- [ ] **Step 2:** Implement `PluginManager` struct with per-user McpPool map, SkillLoader, WASM host cache
- [ ] **Step 3:** Implement `tools_for_turn()` — queries plugins table, builds tools from each kind
- [ ] **Step 4:** Implement `mcp_tools()` — reads MCP plugin configs, delegates to McpPool
- [ ] **Step 5:** Implement `skill_tool()` — reads skill plugin configs, delegates to SkillLoader
- [ ] **Step 6:** Implement `wasm_tools()` — reads WASM plugin configs, loads PluginHost, creates PluginTools
- [ ] **Step 7:** Tests, commit

---

## Task 4: Rewire ToolBootstrap

**Files:**
- Modify: `backend/crates/sober-agent/src/tools/bootstrap.rs`
- Modify: `backend/crates/sober-agent/Cargo.toml`

Replace direct `McpPool` + `SkillLoader` with `PluginManager`.

- [ ] **Step 1:** Add `sober-plugin` dependency to sober-agent
- [ ] **Step 2:** Replace `skill_loader` field with `plugin_manager` in `ToolBootstrap`
- [ ] **Step 3:** Update `build()` to call `plugin_manager.tools_for_turn()`
- [ ] **Step 4:** Remove direct MCP pool management from agent loop
- [ ] **Step 5:** Fix all compilation errors
- [ ] **Step 6:** Run `cargo test -p sober-agent -q`, commit

---

## Task 5: Add plugin gRPC RPCs

**Files:**
- Modify/Create: `backend/proto/sober/agent/v1/agent.proto` or `plugin.proto`

Add RPCs for plugin management (called by API and CLI):

```protobuf
rpc ListPlugins(ListPluginsRequest) returns (ListPluginsResponse);
rpc InstallPlugin(InstallPluginRequest) returns (InstallPluginResponse);
rpc UninstallPlugin(UninstallPluginRequest) returns (UninstallPluginResponse);
rpc EnablePlugin(EnablePluginRequest) returns (EnablePluginResponse);
rpc DisablePlugin(DisablePluginRequest) returns (DisablePluginResponse);
rpc ImportPlugins(ImportPluginsRequest) returns (ImportPluginsResponse);
rpc ReloadPlugins(ReloadPluginsRequest) returns (ReloadPluginsResponse);
```

- [ ] **Step 1:** Define proto messages and service RPCs
- [ ] **Step 2:** Implement gRPC handlers in sober-agent
- [ ] **Step 3:** Tests, commit

---

## Task 6: Unified plugin API routes

**Files:**
- Create: `backend/crates/sober-api/src/routes/plugins.rs`
- Modify: `backend/crates/sober-api/src/routes/mod.rs`

Replace `/api/v1/mcp/servers` and `/api/v1/skills` with `/api/v1/plugins`.
Routes proxy to agent via gRPC.

- [ ] **Step 1:** Implement `GET /api/v1/plugins` (list with filters)
- [ ] **Step 2:** Implement `POST /api/v1/plugins` (install)
- [ ] **Step 3:** Implement `POST /api/v1/plugins/import` (batch config import)
- [ ] **Step 4:** Implement `GET /api/v1/plugins/:id` (details)
- [ ] **Step 5:** Implement `PATCH /api/v1/plugins/:id` (enable/disable, update config)
- [ ] **Step 6:** Implement `DELETE /api/v1/plugins/:id` (uninstall)
- [ ] **Step 7:** Implement `GET /api/v1/plugins/:id/audit` (audit report)
- [ ] **Step 8:** Implement `POST /api/v1/plugins/reload` (re-scan skills)
- [ ] **Step 9:** Remove old MCP and Skills routes
- [ ] **Step 10:** Tests, commit

---

## Task 7: Scaffold sober-plugin-gen crate

**Files:**
- Create: `backend/crates/sober-plugin-gen/Cargo.toml`
- Create: `backend/crates/sober-plugin-gen/src/lib.rs`
- Create: `backend/crates/sober-plugin-gen/src/error.rs`
- Modify: `backend/Cargo.toml` (add to workspace members)

- [ ] **Step 1:** Create crate with dependencies (sober-core, sober-plugin, sober-llm)
- [ ] **Step 2:** Implement `GenError` enum
- [ ] **Step 3:** Verify `cargo build -p sober-plugin-gen -q`
- [ ] **Step 4:** Commit

---

## Task 8: Template scaffolding

**Files:**
- Create: `backend/crates/sober-plugin-gen/src/scaffold.rs`

`scaffold()` generates a plugin skeleton (plugin.toml, Cargo.toml, src/lib.rs,
build.rs) in a target directory. No LLM needed.

- [ ] **Step 1:** Write tests (scaffold creates expected files, files parse correctly)
- [ ] **Step 2:** Implement `scaffold(name, output_dir)` with embedded templates
- [ ] **Step 3:** Run tests, commit

---

## Task 9: WASM compilation

**Files:**
- Create: `backend/crates/sober-plugin-gen/src/compile.rs`

Shell out to `cargo build --target wasm32-wasi --release` in the plugin
source directory. Read the output `.wasm` file and return bytes.

- [ ] **Step 1:** Write tests (compile a scaffolded plugin, verify .wasm output)
- [ ] **Step 2:** Implement `compile(source_dir) -> Result<Vec<u8>, GenError>`
- [ ] **Step 3:** Run tests, commit

---

## Task 10: LLM-powered generation

**Files:**
- Create: `backend/crates/sober-plugin-gen/src/generate.rs`

The self-correcting generation loop: prompt LLM → parse source → compile →
test → retry on failure (max 3 attempts).

- [ ] **Step 1:** Write tests with a mock `LlmEngine` that returns pre-written source
- [ ] **Step 2:** Implement `PluginGenerator::generate_wasm()`
- [ ] **Step 3:** Implement `PluginGenerator::generate_skill()`
- [ ] **Step 4:** Build generation prompts (PDK trait, manifest format, capability API)
- [ ] **Step 5:** Run tests, commit

---

## Task 11: Frontend — unified plugins page

**Files:**
- Create: `frontend/src/routes/(app)/settings/plugins/+page.svelte`
- Create: `frontend/src/lib/services/plugins.ts`
- Create: `frontend/src/lib/types/plugin.ts`
- Delete: `frontend/src/routes/(app)/settings/mcp/+page.svelte`
- Delete: `frontend/src/lib/services/mcp.ts`

- [ ] **Step 1:** Define TypeScript types mirroring backend Plugin types
- [ ] **Step 2:** Implement `plugins.ts` service (list, install, import, update, delete)
- [ ] **Step 3:** Build plugins page with kind filter tabs (All / MCP / Skills / WASM)
- [ ] **Step 4:** MCP install form (name, command, args, env)
- [ ] **Step 5:** Config file import (paste/upload `.mcp.json`)
- [ ] **Step 6:** Enable/disable toggles, uninstall buttons
- [ ] **Step 7:** Audit report view
- [ ] **Step 8:** Remove old MCP settings page and service
- [ ] **Step 9:** Update navigation links
- [ ] **Step 10:** Run `pnpm check` and `pnpm test --silent`
- [ ] **Step 11:** Commit

---

## Task 12: /generate-plugin slash command

Wire the agent to expose `/generate-plugin` as a user-invocable command
that triggers `sober-plugin-gen` in the current workspace.

- [ ] **Step 1:** Add `generate-plugin` as an agent tool or slash command handler
- [ ] **Step 2:** Parse the description from user input
- [ ] **Step 3:** Call `PluginGenerator::generate_wasm()` with workspace scope
- [ ] **Step 4:** Install the generated plugin via `PluginManager.install()`
- [ ] **Step 5:** Return confirmation message to the user
- [ ] **Step 6:** Tests, commit

---

## Task 13: Final verification

- [ ] **Step 1:** `cargo build -q` (full workspace)
- [ ] **Step 2:** `cargo test --workspace -q`
- [ ] **Step 3:** `cargo clippy --workspace -q -- -D warnings`
- [ ] **Step 4:** `pnpm check` and `pnpm test --silent` (frontend)
- [ ] **Step 5:** End-to-end test: install MCP plugin via API, verify it appears in agent tools
- [ ] **Step 6:** End-to-end test: `/generate-plugin` creates and enables a WASM plugin

---

## Acceptance Criteria

- [ ] `PluginManager.tools_for_turn()` returns MCP + Skill + WASM tools
- [ ] `ToolBootstrap` uses `PluginManager` (no direct McpPool/SkillLoader)
- [ ] `mcp_servers` table migrated to `plugins` and dropped
- [ ] `McpServerRepo`, `McpServerId`, `McpServerConfig` removed from sober-core
- [ ] gRPC RPCs for plugin CRUD accessible from API and CLI
- [ ] `/api/v1/plugins` routes replace `/api/v1/mcp/servers` and `/api/v1/skills`
- [ ] `.mcp.json` config file import works via `POST /api/v1/plugins/import`
- [ ] Frontend `/settings/plugins` page with kind filter tabs
- [ ] `sober-plugin-gen` scaffolds plugin templates
- [ ] `sober-plugin-gen` compiles Rust plugins to WASM
- [ ] `sober-plugin-gen` generates plugins via LLM with self-correcting retry loop
- [ ] `/generate-plugin` slash command works end-to-end
- [ ] No regressions in existing tests
