CREATE TABLE mcp_servers (
    id         UUID PRIMARY KEY,
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    command    TEXT NOT NULL,
    args       JSONB NOT NULL DEFAULT '[]',
    env        JSONB NOT NULL DEFAULT '{}',
    enabled    BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT uq_mcp_servers_user_name UNIQUE (user_id, name)
);

CREATE INDEX idx_mcp_servers_user_id ON mcp_servers (user_id);
CREATE INDEX idx_mcp_servers_enabled ON mcp_servers (user_id, enabled) WHERE enabled = true;
