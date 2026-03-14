-- Only one active job per (name, owner_id) combination.
CREATE UNIQUE INDEX idx_jobs_unique_active_name_owner
    ON jobs (name, owner_id) WHERE status = 'active';
