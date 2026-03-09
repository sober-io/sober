-- Stores MEK-wrapped DEKs (data encryption keys) per user scope.
-- Each user gets one DEK for encrypting all their secrets.
CREATE TABLE encryption_keys (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    encrypted_dek BYTEA NOT NULL,
    mek_version   INT NOT NULL DEFAULT 1,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    rotated_at    TIMESTAMPTZ
);

-- One DEK per user (enforced by unique index).
CREATE UNIQUE INDEX idx_encryption_keys_user ON encryption_keys(user_id);
