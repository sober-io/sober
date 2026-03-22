-- Migrate mcp_servers data into the unified plugins table.
-- Part of plan #019: Unified Plugin System
--
-- Column mapping:
--   mcp_servers.id         → plugins.id
--   mcp_servers.name       → plugins.name
--   (literal)              → plugins.kind      = 'mcp'
--   (NULL)                 → plugins.version
--   (NULL)                 → plugins.description
--   (literal)              → plugins.origin     = 'user'
--   (literal)              → plugins.scope      = 'user'
--   mcp_servers.user_id    → plugins.owner_id + plugins.installed_by
--   (NULL)                 → plugins.workspace_id
--   mcp_servers.enabled    → plugins.status     (true → 'enabled', false → 'disabled')
--   {command, args, env}   → plugins.config
--   mcp_servers.created_at → plugins.installed_at
--   mcp_servers.updated_at → plugins.updated_at

INSERT INTO plugins (
    id,
    name,
    kind,
    version,
    description,
    origin,
    scope,
    owner_id,
    workspace_id,
    status,
    config,
    installed_by,
    installed_at,
    updated_at
)
SELECT
    id,
    name,
    'mcp'::plugin_kind,
    NULL,
    NULL,
    'user'::plugin_origin,
    'user'::plugin_scope,
    user_id,
    NULL,
    CASE WHEN enabled THEN 'enabled'::plugin_status ELSE 'disabled'::plugin_status END,
    jsonb_build_object('command', command, 'args', args, 'env', env),
    user_id,
    created_at,
    updated_at
FROM mcp_servers;

-- Drop indexes before dropping the table (implicit with DROP TABLE, but explicit for clarity)
DROP INDEX IF EXISTS idx_mcp_servers_enabled;
DROP INDEX IF EXISTS idx_mcp_servers_user_id;

DROP TABLE mcp_servers;
