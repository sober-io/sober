-- Self-evolution events table and autonomy configuration.
-- Stores agent-proposed evolutions (plugins, skills, instructions, automations)
-- with lifecycle tracking, deduplication, and configurable autonomy levels.

CREATE TYPE evolution_type AS ENUM ('plugin', 'skill', 'instruction', 'automation');
CREATE TYPE evolution_status AS ENUM (
  'proposed', 'approved', 'executing', 'active', 'failed', 'rejected', 'reverted'
);

CREATE TABLE evolution_events (
  id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  evolution_type  evolution_type NOT NULL,
  user_id         UUID REFERENCES users(id) ON DELETE SET NULL,
  title           TEXT NOT NULL,
  description     TEXT NOT NULL,
  confidence      REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
  source_count    INT NOT NULL DEFAULT 1,
  status          evolution_status NOT NULL DEFAULT 'proposed',
  payload         JSONB NOT NULL,
  result          JSONB,
  status_history  JSONB NOT NULL DEFAULT '[]',
  decided_by      UUID REFERENCES users(id),
  reverted_at     TIMESTAMPTZ,
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_evolution_events_status ON evolution_events(status);
CREATE INDEX idx_evolution_events_type_status ON evolution_events(evolution_type, status);

-- Deduplication: prevent duplicate active evolutions of the same type and title.
-- Uses a functional index that derives a slug from the title at query time.
CREATE UNIQUE INDEX idx_evolution_events_no_duplicates
  ON evolution_events(evolution_type, lower(regexp_replace(title, '[^a-z0-9]+', '-', 'g')))
  WHERE status IN ('proposed', 'approved', 'executing', 'active');

-- Singleton configuration table for per-type autonomy levels.
-- Autonomy levels stored as TEXT (not a PostgreSQL enum) for flexibility.
CREATE TABLE evolution_config (
  id              BOOL PRIMARY KEY DEFAULT TRUE CHECK (id),
  plugin_autonomy TEXT NOT NULL DEFAULT 'approval_required',
  skill_autonomy  TEXT NOT NULL DEFAULT 'auto',
  instruction_autonomy TEXT NOT NULL DEFAULT 'approval_required',
  automation_autonomy TEXT NOT NULL DEFAULT 'auto',
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO evolution_config DEFAULT VALUES;
