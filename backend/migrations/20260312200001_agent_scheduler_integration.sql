-- Migrate existing 'agent' owner_type rows before changing constraint
UPDATE jobs SET owner_type = 'system' WHERE owner_type = 'agent';

-- Add workspace, creator, and conversation tracking
ALTER TABLE jobs ADD COLUMN workspace_id UUID REFERENCES workspaces(id);
ALTER TABLE jobs ADD COLUMN created_by UUID REFERENCES users(id);
ALTER TABLE jobs ADD COLUMN conversation_id UUID REFERENCES conversations(id) ON DELETE SET NULL;

-- Expand owner_type to include 'group', drop unused 'agent'
ALTER TABLE jobs DROP CONSTRAINT jobs_owner_type_check;
ALTER TABLE jobs ADD CONSTRAINT jobs_owner_type_check
    CHECK (owner_type IN ('system', 'user', 'group'));

-- Add result artifact reference to job_runs
ALTER TABLE job_runs ADD COLUMN result_artifact_ref TEXT;

-- Index for workspace-scoped queries
CREATE INDEX idx_jobs_workspace ON jobs(workspace_id) WHERE workspace_id IS NOT NULL;

-- Remove redundant binary payload column (consolidated onto payload JSONB)
ALTER TABLE jobs DROP COLUMN IF EXISTS payload_bytes;
