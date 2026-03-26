# 042: Workspace Settings

## Problem

Workspace configuration is split across two mechanisms:

1. **DB columns on `conversations`** — `permission_mode` lives on the conversation
   table, but it's a workspace concern (workspace:conversation is 1:1).
2. **File-based `.sober/config.toml`** — sandbox settings, auto_snapshot, and other
   workspace config live in the filesystem. Error-prone (TOML syntax), no UI, and
   the agent never actually reads the sandbox section.

The agent resolves sandbox policy once at startup from the system config
(`config.agent.sandbox_profile`) and applies it to all conversations. Per-workspace
sandbox customization has no effect.

## Solution

Introduce a `workspace_settings` table as the single source of truth for all
workspace-level configuration. Move `permission_mode` off conversations. Store
sandbox settings as structured, typed columns — not JSONB. Wire the agent to
resolve sandbox policy from DB settings instead of only using the startup default.

## Schema

```sql
CREATE TYPE sandbox_net_mode AS ENUM ('none', 'allowed_domains', 'full');

CREATE TABLE workspace_settings (
    workspace_id                  UUID PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    permission_mode               permission_mode NOT NULL DEFAULT 'policy_based',
    auto_snapshot                 BOOLEAN NOT NULL DEFAULT true,
    max_snapshots                 INTEGER,
    sandbox_profile               TEXT NOT NULL DEFAULT 'standard',
    sandbox_net_mode              sandbox_net_mode,
    sandbox_allowed_domains       TEXT[],
    sandbox_max_execution_seconds INTEGER,
    sandbox_allow_spawn           BOOLEAN,
    created_at                    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                    TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

`sandbox_profile` is TEXT — built-in names (`standard`, `locked_down`,
`unrestricted`) plus custom profile names referencing entries in the system
config's `[sandbox.profiles]` section. Validated at the application layer.

`sandbox_net_mode` is a Postgres enum — closed set, no custom variants.

Nullable sandbox override columns mean "use whatever the profile provides."
Only non-null values override the profile defaults.

## Key Decisions

### Workspace:Conversation is 1:1

Every conversation has exactly one workspace. No inheritance or sharing.

### Workspace + settings created with conversation

`POST /conversations` provisions workspace + workspace_settings + conversation
atomically. No lazy creation needed in the normal path.

A shared `WorkspaceRepo::provision()` method handles the creation logic so that
the agent's `ensure_workspace()` fallback (for pre-migration conversations) uses
the same code path.

### Settings endpoint

```
GET   /conversations/{id}/settings → full settings state
PATCH /conversations/{id}/settings → partial update, omitted fields unchanged
```

PATCH supports both quick toggles (e.g. `{ "sandbox_profile": "unrestricted" }`)
and full panel saves. The backend splits the payload across `conversations`
(agent_mode) and `workspace_settings` (permission_mode, sandbox, auto_snapshot)
in one transaction.

Conversation mutations (title, archive) stay on `PATCH /conversations/{id}`.

#### Request/Response shape

```json
{
  "permission_mode": "policy_based",
  "agent_mode": "always",
  "sandbox_profile": "standard",
  "sandbox_net_mode": "allowed_domains",
  "sandbox_allowed_domains": ["github.com"],
  "sandbox_max_execution_seconds": 120,
  "sandbox_allow_spawn": false,
  "auto_snapshot": true
}
```

GET returns all fields. PATCH accepts any subset — omitted fields are not changed.

### File-based config reduced in scope

`.sober/config.toml` can still exist for repo-level hints (style, commit
conventions) but is no longer used for sandbox, permission, or snapshot settings.

## Changes

### 1. Migration

- Create `sandbox_profile` and `sandbox_net_mode` enums.
- Create `workspace_settings` table.
- Migrate `permission_mode` data from `conversations` to `workspace_settings`.
- Drop `permission_mode` column from `conversations`.

### 2. Domain types (`sober-core`)

- Add `WorkspaceSettings` domain struct matching the table.
- Add `WorkspaceSettingsRepo` trait (get, upsert).
- Remove `permission_mode` from conversation domain types.

### 3. DB layer (`sober-db`)

- `PgWorkspaceSettingsRepo` implementation.
- `WorkspaceRepo::provision()` — creates workspace + settings atomically.
- Update conversation queries to stop selecting/inserting `permission_mode`.

### 4. API layer (`sober-api`)

- `POST /conversations` creates workspace + settings + conversation atomically.
  No `workspace_id` in request body.
- `GET /conversations/{id}/settings` returns combined settings.
- `PATCH /conversations/{id}/settings` partial update, both tables in one tx.
- Remove `permission_mode`, `workspace_id`, `agent_mode` from `PATCH /conversations/{id}`.
- Remove or deprecate `GET/PUT /workspaces/{id}/settings` (file-based).

### 5. Agent

- `ensure_workspace()` simplified: normal path just resolves dir + loads settings.
  Fallback calls `provision()` for pre-migration conversations.
- `TurnContext` carries `WorkspaceSettings`.
- `ToolBootstrap::build()` resolves `SandboxPolicy` from workspace settings.
- Falls back to system startup policy if no settings exist.

### 6. Frontend

- Update settings panel to include sandbox fields.
- Single `PATCH /conversations/{id}/settings` call on save.
- Quick toggle buttons use same PATCH with single field.
- Remove `permission_mode` from `Conversation` type.

## What stays the same

- System-level `sandbox_profile` in agent config remains the fallback.
- `SandboxProfile::resolve()` and `SandboxPolicy` types in `sober-sandbox` unchanged.
- bwrap execution, audit logging, command classification unaffected.
- `PATCH /conversations/{id}` still handles title, archive, tags, collaborators.

## Testing

- Unit test: resolve sandbox policy from `WorkspaceSettings` with various
  override combinations.
- Integration test: `POST /conversations` creates workspace + settings atomically.
- Integration test: `POST /conversations/{id}/settings` round-trip.
- Integration test: agent resolves correct sandbox policy from DB settings.
- Integration test: agent fallback creates workspace for pre-migration conversations.
