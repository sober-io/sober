-- Rename table
ALTER TABLE user_secrets RENAME TO secrets;

-- Add conversation scope
ALTER TABLE secrets ADD COLUMN conversation_id UUID REFERENCES conversations(id);

-- Rebuild unique constraints for dual-scope naming
DROP INDEX IF EXISTS idx_user_secrets_name;
CREATE UNIQUE INDEX idx_secrets_name
  ON secrets (user_id, name) WHERE conversation_id IS NULL;
CREATE UNIQUE INDEX idx_secrets_conversation_name
  ON secrets (conversation_id, name) WHERE conversation_id IS NOT NULL;
CREATE INDEX idx_secrets_conversation ON secrets (conversation_id)
  WHERE conversation_id IS NOT NULL;

-- Rename leftover indexes for consistency
ALTER INDEX IF EXISTS idx_user_secrets_user RENAME TO idx_secrets_user;
ALTER INDEX IF EXISTS idx_user_secrets_type RENAME TO idx_secrets_type;
