-- Stores encrypted secrets scoped to users.
-- Secret values are encrypted with the user's DEK (AES-256-GCM).
-- Metadata (name, secret_type, JSON metadata) is stored in plaintext
-- for searchability; only the sensitive payload is encrypted.
CREATE TABLE user_secrets (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id        UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    secret_type    TEXT NOT NULL,
    metadata       JSONB NOT NULL DEFAULT '{}',
    encrypted_data BYTEA NOT NULL,
    priority       INT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Index for listing secrets by user.
CREATE INDEX idx_user_secrets_user ON user_secrets(user_id);

-- Index for filtering by secret type.
CREATE INDEX idx_user_secrets_type ON user_secrets(user_id, secret_type);

-- Unique name per user to prevent duplicate secret labels.
CREATE UNIQUE INDEX idx_user_secrets_name ON user_secrets(user_id, name);
