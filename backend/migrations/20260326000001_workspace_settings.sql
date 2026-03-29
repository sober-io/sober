-- Plan 042: Workspace settings — single source of truth for workspace-level configuration.
--
-- Changes:
--   1. New enum types: permission_mode, sandbox_net_mode
--   2. New table: workspace_settings
--   3. Migrate permission_mode data from conversations → workspace_settings
--   4. Drop permission_mode column from conversations

-- ============================================================
-- 1. Enum types
-- ============================================================

CREATE TYPE permission_mode AS ENUM ('interactive', 'policy_based', 'autonomous');
CREATE TYPE sandbox_net_mode AS ENUM ('none', 'allowed_domains', 'full');

-- ============================================================
-- 2. workspace_settings table
-- ============================================================

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

-- ============================================================
-- 3. Migrate permission_mode from conversations to workspace_settings
-- ============================================================

-- For each conversation that has a workspace_id, create a workspace_settings
-- row carrying its permission_mode. If multiple conversations share a workspace
-- (shouldn't happen, but defensively), take the first one.
INSERT INTO workspace_settings (workspace_id, permission_mode)
SELECT DISTINCT ON (c.workspace_id)
    c.workspace_id,
    c.permission_mode::permission_mode
FROM conversations c
WHERE c.workspace_id IS NOT NULL
ON CONFLICT (workspace_id) DO NOTHING;

-- For workspaces that have no conversation yet, insert default settings.
INSERT INTO workspace_settings (workspace_id)
SELECT w.id
FROM workspaces w
WHERE NOT EXISTS (
    SELECT 1 FROM workspace_settings ws WHERE ws.workspace_id = w.id
);

-- ============================================================
-- 4. Drop permission_mode from conversations
-- ============================================================

ALTER TABLE conversations DROP COLUMN permission_mode;
