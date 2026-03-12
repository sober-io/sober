-- Extend the jobs table for the autonomous scheduler.

-- Add owner tracking (who created the job).
ALTER TABLE jobs ADD COLUMN owner_type TEXT NOT NULL DEFAULT 'system'
    CHECK (owner_type IN ('system', 'user', 'agent'));
ALTER TABLE jobs ADD COLUMN owner_id UUID;

-- Add 'running' to the job_status enum so the scheduler can mark in-flight jobs.
ALTER TYPE job_status ADD VALUE IF NOT EXISTS 'running';

-- Make next_run_at non-nullable (scheduler needs it for all jobs).
ALTER TABLE jobs ALTER COLUMN next_run_at SET NOT NULL;

-- Payload as BYTEA (opaque bytes for the scheduler) — add alongside existing JSONB.
-- The scheduler uses payload_bytes; the existing payload (JSONB) stays for backward compat.
ALTER TABLE jobs ADD COLUMN payload_bytes BYTEA NOT NULL DEFAULT '';

-- Index for listing jobs by owner.
CREATE INDEX IF NOT EXISTS idx_jobs_owner ON jobs (owner_type, owner_id);

-- Job run history for tracking execution outcomes.
CREATE TABLE job_runs (
    id          UUID PRIMARY KEY,
    job_id      UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    started_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    status      TEXT NOT NULL DEFAULT 'running' CHECK (status IN ('running', 'succeeded', 'failed')),
    result      BYTEA NOT NULL DEFAULT '',
    error       TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Index for looking up runs by job.
CREATE INDEX idx_job_runs_job_id ON job_runs (job_id, started_at DESC);
