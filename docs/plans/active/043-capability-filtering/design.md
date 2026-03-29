# 043: Workspace Capability Filtering

## Problem

All tools and plugins are available in every workspace. There is no mechanism to
restrict what the agent can use per workspace — neither for security (e.g., no
shell in a sensitive workspace) nor for UX (e.g., hiding irrelevant MCP tools in
a frontend-only project).

Plugins can be globally enabled/disabled via `PATCH /plugins/{id}`, but that
affects all workspaces. Built-in tools (shell, web_search, etc.) cannot be
disabled at all.

## Solution

Blacklist model on `workspace_settings` (042). Everything enabled by default;
users explicitly disable entire plugins (by UUID) or individual tools (by name)
per workspace. Disabled capabilities are silently excluded — the agent never
sees them.

### Why blacklist over whitelist

- **Low friction** — new plugins and tools work immediately without per-workspace
  opt-in.
- **Whitelist alternative** — too much maintenance: every new plugin must be
  manually enabled in every workspace that needs it.
- **Hybrid (mode switch)** — YAGNI. If needed later, add a `tool_access_mode`
  column (`open` / `restricted`). Backwards-compatible; no schema redesign.

## Schema

Two columns added to `workspace_settings` (042's table):

```sql
ALTER TABLE workspace_settings
  ADD COLUMN disabled_tools   TEXT[]  NOT NULL DEFAULT '{}',
  ADD COLUMN disabled_plugins UUID[]  NOT NULL DEFAULT '{}';
```

- `disabled_tools` — tool names (built-in or plugin-exported). E.g.,
  `["shell", "web_search", "mcp__postgres__query"]`.
- `disabled_plugins` — plugin UUIDs. Disables ALL tools exported by that plugin.

No per-tool granularity within plugins for the `disabled_plugins` list — disabling
a plugin disables everything it provides. For finer control, use `disabled_tools`
with the specific tool name from that plugin.

## Enforcement

Two-pass filter applied during tool resolution each turn:

1. **Plugin filter** — `PluginManager::tools_for_turn()` skips plugins whose ID
   is in `disabled_plugins`. All tools from those plugins are excluded.
2. **Tool name filter** — After collecting all tools (built-in from
   `ToolBootstrap` + remaining plugin tools), remove any whose name is in
   `disabled_tools`.

Both filters read `workspace_settings` from `TurnContext` (added by 042). Silent
exclusion — disabled tools are simply not registered for the turn.

## gRPC

New RPC on the agent service (separate from existing `ListPlugins`):

```protobuf
rpc ListTools(ListToolsRequest) returns (ListToolsResponse);

message ListToolsRequest {
  string user_id = 1;
  optional string workspace_id = 2;
}

message ToolInfo {
  string name = 1;
  string description = 2;
  string source = 3;           // "builtin" or "plugin"
  optional string plugin_id = 4;
  optional string plugin_name = 5;
}

message ListToolsResponse {
  repeated ToolInfo tools = 1;
}
```

Returns the **unfiltered** catalog of all tools (built-in + plugin-exported)
regardless of disabled lists. The frontend compares against
`workspace_settings.disabled_tools` to render enabled/disabled state.

`ListPlugins` (existing) remains for plugin management. `ListTools` answers a
different question: "what tools exist?" vs "what plugins are installed?"

## API

**New endpoint:**
- `GET /api/v1/tools` — proxies `ListTools` gRPC. Returns all tools with name,
  description, and source attribution.

**Modified endpoints (from 042):**
- `GET /conversations/{id}/settings` — response includes `disabled_tools` and
  `disabled_plugins`.
- `PATCH /conversations/{id}/settings` — accepts `disabled_tools: string[]` and
  `disabled_plugins: string[]` (UUIDs as strings).

**Existing endpoints used by UI:**
- `GET /plugins` — lists plugins for the "disable whole plugin" toggle section.

## Frontend

New **"Capabilities"** section in workspace settings panel:

1. **Plugins** — toggle switches for each plugin (from `GET /plugins`).
   Off = UUID added to `disabled_plugins`.
2. **Tools** — toggle switches for each tool (from `GET /tools`).
   Off = name added to `disabled_tools`. Includes built-in and plugin-exported
   tools.
3. **Free-text input** — power user field to type arbitrary tool names into
   `disabled_tools`.
