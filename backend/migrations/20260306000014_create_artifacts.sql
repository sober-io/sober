CREATE TABLE artifacts (
    id           UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    kind         artifact_kind NOT NULL,
    state        artifact_state NOT NULL DEFAULT 'draft',
    path         TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_artifacts_workspace_id ON artifacts (workspace_id);
CREATE INDEX idx_artifacts_kind ON artifacts (workspace_id, kind);
CREATE INDEX idx_artifacts_state ON artifacts (workspace_id, state);
