# 020 — Secrets Management

**Date:** 2026-03-07
**Status:** Pending
**Scope:** Envelope encryption for user/group secrets, LLM key resolution

CI/CD workflows and `sober-web` crate are covered in [002 — Project Skeleton](../002-project-skeleton/design.md).

---

## Problem

v1 stores LLM API keys as global env vars and MCP credentials as plaintext in the database. Users and groups cannot bring their own provider keys, and there is no encrypted storage for arbitrary secrets.

## Design

### Encryption Model: Server-Side Envelope Encryption

Two-layer key hierarchy:

- **Master Encryption Key (MEK)** — loaded from env var at startup. The single externally-managed secret.
- **Data Encryption Key (DEK)** — one per user/group, generated randomly (256-bit). Stored in DB wrapped (encrypted) by the MEK.

All secret values are encrypted by the owning scope's DEK using AES-256-GCM. Each encryption produces a fresh random 12-byte nonce.

```
MEK (env var)
 └── wraps DEK_alice (stored in DB)
      └── encrypts alice's secrets
 └── wraps DEK_team1 (stored in DB)
      └── encrypts team1's secrets
```

**What this protects against:** database-only breach, leaked backups, SQL injection. Does NOT protect against a compromised running server (server holds MEK in memory).

### Crypto Primitives (`sober-crypto`)

New module extending existing crate:

```rust
pub struct Mek(/* [u8; 32] */);
pub struct Dek(/* [u8; 32] */);

pub struct EncryptedBlob {
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
}

impl Mek {
    pub fn from_env(hex_key: &str) -> Result<Self, CryptoError>;
    pub fn wrap_dek(&self, dek: &Dek) -> Result<EncryptedBlob, CryptoError>;
    pub fn unwrap_dek(&self, blob: &EncryptedBlob) -> Result<Dek, CryptoError>;
}

impl Dek {
    pub fn generate() -> Result<Self, CryptoError>;
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedBlob, CryptoError>;
    pub fn decrypt(&self, blob: &EncryptedBlob) -> Result<Vec<u8>, CryptoError>;
}
```

Both types use AES-256-GCM from `aws-lc-rs`. MEK wraps DEKs, DEKs wrap data — the types enforce correct usage.

### Database Schema (`sober-db`)

```sql
CREATE TABLE encryption_keys (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID REFERENCES users(id),
    group_id      UUID REFERENCES groups(id),
    encrypted_dek BYTEA NOT NULL,
    mek_version   INT NOT NULL DEFAULT 1,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    rotated_at    TIMESTAMPTZ,
    CONSTRAINT one_scope CHECK (
        (user_id IS NOT NULL AND group_id IS NULL) OR
        (user_id IS NULL AND group_id IS NOT NULL)
    )
);

CREATE TABLE user_secrets (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id        UUID REFERENCES users(id),
    group_id       UUID REFERENCES groups(id),
    name           TEXT NOT NULL,
    secret_type    TEXT NOT NULL,
    metadata       JSONB NOT NULL DEFAULT '{}',
    encrypted_data BYTEA NOT NULL,
    priority       INT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT one_scope CHECK (
        (user_id IS NOT NULL AND group_id IS NULL) OR
        (user_id IS NULL AND group_id IS NOT NULL)
    )
);
```

**Fields:**

- `name` — human-readable label (e.g., "My Anthropic Key")
- `secret_type` — category string: `llm_provider`, `oauth_app`, `api_token`, etc.
- `metadata` — plaintext JSON for non-sensitive fields (e.g., `client_id`, `base_url`, `provider`). Searchable, displayed without decryption.
- `encrypted_data` — AES-256-GCM ciphertext of a JSON blob containing only the sensitive fields (e.g., `{ "api_key": "sk-..." }` or `{ "client_secret": "ghs_..." }`)
- `priority` — ordering for provider fallback chains (lower = higher priority). NULL for non-ordered secrets.

### Example Data

```
Alice's primary LLM key:
  secret_type: 'llm_provider', priority: 1
  metadata: { "provider": "anthropic", "base_url": "https://api.anthropic.com/v1" }
  encrypted_data: encrypt({ "api_key": "sk-ant-..." })

Alice's fallback LLM key:
  secret_type: 'llm_provider', priority: 2
  metadata: { "provider": "openrouter", "base_url": "https://openrouter.ai/api/v1" }
  encrypted_data: encrypt({ "api_key": "sk-or-..." })

Alice's GitHub OAuth app:
  secret_type: 'oauth_app', priority: NULL
  metadata: { "client_id": "Iv1.abc123", "scope": "repo,issues" }
  encrypted_data: encrypt({ "client_secret": "ghs_..." })

Team1's shared LLM key:
  secret_type: 'llm_provider', priority: 1
  metadata: { "provider": "openai", "base_url": "https://api.openai.com/v1" }
  encrypted_data: encrypt({ "api_key": "sk-..." })
```

### Repository Trait (`sober-core`)

```rust
#[async_trait]
pub trait SecretRepo {
    async fn get_dek(&self, scope: SecretScope) -> Result<Option<EncryptedBlob>>;
    async fn store_dek(&self, scope: SecretScope, dek: EncryptedBlob, mek_version: i32) -> Result<()>;

    async fn list_secrets(&self, scope: SecretScope, secret_type: Option<&str>) -> Result<Vec<SecretMetadata>>;
    async fn get_secret(&self, id: Uuid) -> Result<Option<SecretRow>>;
    async fn store_secret(&self, secret: NewSecret) -> Result<Uuid>;
    async fn update_secret(&self, id: Uuid, secret: UpdateSecret) -> Result<()>;
    async fn delete_secret(&self, id: Uuid) -> Result<()>;
}

pub enum SecretScope {
    User(UserId),
    Group(GroupId),
}
```

### LLM Key Resolution

Three-tier resolution when the agent needs an LLM provider key:

```
1. User's secrets (secret_type = 'llm_provider', ORDER BY priority)
2. User's groups' secrets (same filter, ordered by priority)
3. System config (AppConfig.llm — env vars)
```

First available key is used. On provider failure (rate limit, auth error), try next in chain. Resolution logic lives in `sober-llm` as a service that takes `SecretRepo` + `Mek`.

The system built-in provider chain follows the same multi-provider-with-priority model but is purely config-based (env vars / config file), not stored in the database.

### Key Rotation

**DEK rotation (per-user, automated):**
- `sober-scheduler` job, configurable interval
- Generate new DEK, decrypt all user's secrets with old DEK, re-encrypt with new DEK
- Re-wrap new DEK with current MEK
- Single DB transaction per user

**MEK rotation (system-wide, admin-initiated):**
- Triggered via `soberctl`
- Load old MEK + new MEK simultaneously
- Re-wrap all DEKs with new MEK (only DEK rows touched, not secret rows)
- `mek_version` column tracks which MEK wrapped each DEK
- `sober-scheduler` job to migrate remaining DEKs to latest version
- Retire old MEK once all DEKs migrated

---

## Architecture Impact

### Config Additions

| Variable | Purpose |
|----------|---------|
| `MASTER_ENCRYPTION_KEY` | Hex-encoded 256-bit MEK for envelope encryption |
