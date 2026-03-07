CREATE TABLE worktrees (
    id         UUID PRIMARY KEY,
    repo_id    UUID NOT NULL REFERENCES workspace_repos (id) ON DELETE CASCADE,
    branch     TEXT NOT NULL,
    path       TEXT NOT NULL,
    stale      BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_worktrees_repo_id ON worktrees (repo_id);
CREATE INDEX idx_worktrees_stale ON worktrees (stale, created_at)
    WHERE stale = false;
