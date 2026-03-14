-- Only one non-terminal job per (name, owner_id) combination.
-- Covers both 'active' and 'running' states to prevent race conditions.
CREATE UNIQUE INDEX idx_jobs_unique_active_name_owner
    ON jobs (name, owner_id) WHERE status IN ('active', 'running');
