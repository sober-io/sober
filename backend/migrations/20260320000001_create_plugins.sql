-- Plugin system tables
-- Part of plan #019: Unified Plugin System
--
-- Note: IDs are generated as v7 UUIDs by the application (Uuid::now_v7()),
-- not by the DB default (gen_random_uuid() produces v4). The DEFAULT clause
-- is a fallback for manual inserts only.

CREATE TYPE plugin_kind AS ENUM ('mcp', 'skill', 'wasm');
CREATE TYPE plugin_origin AS ENUM ('system', 'agent', 'user');
CREATE TYPE plugin_scope AS ENUM ('system', 'user', 'workspace');
CREATE TYPE plugin_status AS ENUM ('enabled', 'disabled', 'failed');

CREATE TABLE plugins (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name           TEXT NOT NULL,
    kind           plugin_kind NOT NULL,
    version        TEXT,
    description    TEXT,
    origin         plugin_origin NOT NULL DEFAULT 'user',
    scope          plugin_scope NOT NULL,
    owner_id       UUID REFERENCES users(id),
    workspace_id   UUID REFERENCES workspaces(id),
    status         plugin_status NOT NULL DEFAULT 'enabled',
    config         JSONB NOT NULL DEFAULT '{}',
    installed_by   UUID REFERENCES users(id),
    installed_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_plugins_unique_name ON plugins (
    name, scope,
    COALESCE(owner_id, '00000000-0000-0000-0000-000000000000'),
    COALESCE(workspace_id, '00000000-0000-0000-0000-000000000000')
);

CREATE INDEX idx_plugins_owner ON plugins (owner_id) WHERE owner_id IS NOT NULL;
CREATE INDEX idx_plugins_workspace ON plugins (workspace_id) WHERE workspace_id IS NOT NULL;
CREATE INDEX idx_plugins_kind_status ON plugins (kind, status);

CREATE TABLE plugin_audit_logs (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plugin_id        UUID REFERENCES plugins(id) ON DELETE SET NULL,
    plugin_name      TEXT NOT NULL,
    kind             plugin_kind NOT NULL,
    origin           plugin_origin NOT NULL,
    stages           JSONB NOT NULL,
    verdict          TEXT NOT NULL,
    rejection_reason TEXT,
    audited_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    audited_by       UUID REFERENCES users(id)
);

CREATE INDEX idx_plugin_audit_logs_plugin ON plugin_audit_logs (plugin_id)
    WHERE plugin_id IS NOT NULL;

CREATE TABLE plugin_kv_data (
    plugin_id  UUID PRIMARY KEY REFERENCES plugins(id) ON DELETE CASCADE,
    data       JSONB NOT NULL DEFAULT '{}',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
