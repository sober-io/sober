CREATE TABLE workspace_repos (
    id              UUID PRIMARY KEY,
    workspace_id    UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    path            TEXT NOT NULL,
    default_branch  TEXT NOT NULL DEFAULT 'main',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT uq_workspace_repos_path UNIQUE (workspace_id, path)
);

CREATE INDEX idx_workspace_repos_workspace_id ON workspace_repos (workspace_id);
