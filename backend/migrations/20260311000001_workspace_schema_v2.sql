-- Plan 017: Workspace, worktree, and artifact schema updates.
--
-- Changes:
--   1. New enum types: workspace_state, worktree_state
--   2. Replace old artifact enum values with new ones
--   3. Add columns to workspaces, workspace_repos, worktrees, artifacts
--   4. Drop old columns replaced by new ones

-- ============================================================
-- 1. New enum types
-- ============================================================

CREATE TYPE workspace_state AS ENUM ('active', 'archived', 'deleted');
CREATE TYPE worktree_state AS ENUM ('active', 'stale', 'removing');

-- ============================================================
-- 2. Replace artifact enums with updated values
-- ============================================================

-- artifact_state: was (draft, committed, archived) → (draft, proposed, approved, rejected, archived)
ALTER TYPE artifact_state RENAME TO artifact_state_old;
CREATE TYPE artifact_state AS ENUM ('draft', 'proposed', 'approved', 'rejected', 'archived');

-- artifact_kind: was (code, document, config, generated) → (code_change, document, proposal, snapshot, trace)
ALTER TYPE artifact_kind RENAME TO artifact_kind_old;
CREATE TYPE artifact_kind AS ENUM ('code_change', 'document', 'proposal', 'snapshot', 'trace');

-- artifact_relation: was (derived_from, supersedes, references) → (spawned_by, supersedes, references, implements)
ALTER TYPE artifact_relation RENAME TO artifact_relation_old;
CREATE TYPE artifact_relation AS ENUM ('spawned_by', 'supersedes', 'references', 'implements');

-- ============================================================
-- 3. Workspaces: add state column, description, created_by, timestamps
-- ============================================================

ALTER TABLE workspaces
    ADD COLUMN description TEXT,
    ADD COLUMN state workspace_state NOT NULL DEFAULT 'active',
    ADD COLUMN created_by UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000' REFERENCES users (id),
    ADD COLUMN archived_at TIMESTAMPTZ,
    ADD COLUMN deleted_at TIMESTAMPTZ;

-- Migrate archived bool → state enum
UPDATE workspaces SET state = 'archived' WHERE archived = true;

-- Drop old archived column and index
DROP INDEX IF EXISTS idx_workspaces_archived;
ALTER TABLE workspaces DROP COLUMN archived;

-- New index on state
CREATE INDEX idx_workspaces_state ON workspaces (user_id, state)
    WHERE state = 'active';

-- ============================================================
-- 4. Workspace repos: add is_linked, remote_url
-- ============================================================

ALTER TABLE workspace_repos
    ADD COLUMN is_linked BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN remote_url TEXT;

-- ============================================================
-- 5. Worktrees: replace stale bool with state enum, add new columns
-- ============================================================

ALTER TABLE worktrees
    ADD COLUMN state worktree_state NOT NULL DEFAULT 'active',
    ADD COLUMN created_by UUID REFERENCES users (id),
    ADD COLUMN task_id UUID,
    ADD COLUMN conversation_id UUID REFERENCES conversations (id),
    ADD COLUMN last_active_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- Migrate stale bool → state enum
UPDATE worktrees SET state = 'stale' WHERE stale = true;

-- Drop old stale column and index
DROP INDEX IF EXISTS idx_worktrees_stale;
ALTER TABLE worktrees DROP COLUMN stale;

-- New index on state
CREATE INDEX idx_worktrees_state ON worktrees (state, created_at)
    WHERE state = 'active';

-- ============================================================
-- 6. Artifacts: rebuild table with new schema
-- ============================================================

-- Drop dependent objects first
DROP INDEX IF EXISTS idx_artifacts_workspace_id;
DROP INDEX IF EXISTS idx_artifacts_kind;
DROP INDEX IF EXISTS idx_artifacts_state;

-- We need to drop artifact_relations temporarily since it references artifacts
-- and we need to change the artifact columns and enum types
ALTER TABLE artifact_relations
    ALTER COLUMN relation TYPE TEXT;

DROP TYPE artifact_relation_old;

ALTER TABLE artifact_relations
    ALTER COLUMN relation TYPE artifact_relation
    USING relation::artifact_relation;

-- Now alter the artifacts table
-- First convert existing enum columns to text temporarily
ALTER TABLE artifacts
    ALTER COLUMN state TYPE TEXT,
    ALTER COLUMN kind TYPE TEXT;

DROP TYPE artifact_state_old;
DROP TYPE artifact_kind_old;

-- Map old values to new values
UPDATE artifacts SET state = 'draft' WHERE state = 'committed';
UPDATE artifacts SET kind = 'code_change' WHERE kind = 'code';
UPDATE artifacts SET kind = 'proposal' WHERE kind = 'config';
UPDATE artifacts SET kind = 'document' WHERE kind = 'generated';

-- Convert back to new enum types
ALTER TABLE artifacts
    ALTER COLUMN state TYPE artifact_state USING state::artifact_state,
    ALTER COLUMN kind TYPE artifact_kind USING kind::artifact_kind;

-- Rename 'name' to 'title', drop 'path', add new columns
ALTER TABLE artifacts RENAME COLUMN name TO title;
ALTER TABLE artifacts DROP COLUMN path;

ALTER TABLE artifacts
    ADD COLUMN user_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000' REFERENCES users (id),
    ADD COLUMN description TEXT,
    ADD COLUMN storage_type TEXT NOT NULL DEFAULT 'inline',
    ADD COLUMN git_repo TEXT,
    ADD COLUMN git_ref TEXT,
    ADD COLUMN blob_key TEXT,
    ADD COLUMN inline_content TEXT,
    ADD COLUMN created_by UUID REFERENCES users (id),
    ADD COLUMN conversation_id UUID REFERENCES conversations (id),
    ADD COLUMN task_id UUID,
    ADD COLUMN parent_id UUID REFERENCES artifacts (id),
    ADD COLUMN reviewed_by UUID REFERENCES users (id),
    ADD COLUMN reviewed_at TIMESTAMPTZ,
    ADD COLUMN metadata JSONB NOT NULL DEFAULT '{}';

-- Recreate indexes
CREATE INDEX idx_artifacts_workspace_id ON artifacts (workspace_id);
CREATE INDEX idx_artifacts_kind ON artifacts (workspace_id, kind);
CREATE INDEX idx_artifacts_state ON artifacts (workspace_id, state);
CREATE INDEX idx_artifacts_user_id ON artifacts (user_id);
