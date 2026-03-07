CREATE TABLE jobs (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    schedule    TEXT NOT NULL,
    status      job_status NOT NULL DEFAULT 'active',
    payload     JSONB NOT NULL DEFAULT '{}',
    next_run_at TIMESTAMPTZ,
    last_run_at TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_jobs_status ON jobs (status);
CREATE INDEX idx_jobs_next_run ON jobs (next_run_at) WHERE status = 'active';
