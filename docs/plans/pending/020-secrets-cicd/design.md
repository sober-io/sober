# 020 — Secrets Management, CI/CD & sober-web

**Date:** 2026-03-07
**Status:** Pending
**Scope:** Envelope encryption for user/group secrets, GitHub Actions CI/CD, new `sober-web` crate

---

## 1. Secrets Management

### Problem

v1 stores LLM API keys as global env vars and MCP credentials as plaintext in the database. Users and groups cannot bring their own provider keys, and there is no encrypted storage for arbitrary secrets.

### Design

#### Encryption Model: Server-Side Envelope Encryption

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

#### Crypto Primitives (`sober-crypto`)

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

#### Database Schema (`sober-db`)

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

#### Example Data

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

#### Repository Trait (`sober-core`)

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

#### LLM Key Resolution

Three-tier resolution when the agent needs an LLM provider key:

```
1. User's secrets (secret_type = 'llm_provider', ORDER BY priority)
2. User's groups' secrets (same filter, ordered by priority)
3. System config (AppConfig.llm — env vars)
```

First available key is used. On provider failure (rate limit, auth error), try next in chain. Resolution logic lives in `sober-llm` as a service that takes `SecretRepo` + `Mek`.

The system built-in provider chain follows the same multi-provider-with-priority model but is purely config-based (env vars / config file), not stored in the database.

#### Key Rotation

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

## 2. CI/CD — GitHub Actions

### Workflows

#### `ci.yml` — Lint, Test, Check

**Triggers:** PR opened/updated, push to main

| Job | Steps |
|-----|-------|
| `rust-check` | `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace`, `cargo audit` |
| `frontend-check` | `pnpm install`, `pnpm check` (svelte-check + tsc), `pnpm lint` (prettier + eslint) |

Runs on `ubuntu-latest`. Fast feedback, no artifacts produced.

#### `release.yml` — Binary Builds

**Triggers:** push tag `v*`

Build matrix:

| Target | Runner |
|--------|--------|
| `x86_64-unknown-linux-gnu` | `ubuntu-latest` |
| `aarch64-unknown-linux-gnu` | `ubuntu-latest` + `cross` |
| `x86_64-apple-darwin` | `macos-latest` |
| `aarch64-apple-darwin` | `macos-latest` |

Produces per-platform archives containing: `sober`, `soberctl`, `sober-api`, `sober-agent`, `sober-scheduler`, `sober-web`. Uploaded as GitHub Release assets.

#### `docker.yml` — Container Images

**Triggers:** push to main (`:latest`), push tag `v*` (`:v0.1.0`)

Four service images, multi-arch (`linux/amd64`, `linux/arm64`):

| Image | Binary |
|-------|--------|
| `ghcr.io/.../sober-api` | API server (headless) |
| `ghcr.io/.../sober-agent` | Agent gRPC server |
| `ghcr.io/.../sober-scheduler` | Tick engine |
| `ghcr.io/.../sober-web` | Static assets + API reverse proxy |

Multi-stage Dockerfile:

```
Stage 1: rust:latest          — cargo build --release
Stage 2: debian:trixie-slim   — copy binary, ca-certificates, create sober user
```

`sober-web` has an additional first stage:

```
Stage 0: node:24-slim          — pnpm install && pnpm build (static output)
Stage 1: rust:latest           — cargo build --release -p sober-web
Stage 2: debian:trixie-slim    — copy binary + copy static assets
```

### Caching

- `actions/cache` for `~/.cargo/registry` and `target/`
- Docker layer caching via `docker/build-push-action` cache-to/cache-from
- Optional `sccache` for Rust compilation caching

### Registry

GHCR (GitHub Container Registry). Free for private repos with GitHub Actions. Seamless transition when repo goes public.

---

## 3. New Crate: `sober-web`

### Responsibility

Single public-facing HTTP entry point. Serves the SvelteKit frontend and reverse-proxies API/WebSocket traffic to `sober-api`.

### Routing

```
Browser -> sober-web (:3000)
              |-- /api/*     -> proxy to sober-api (UDS or localhost)
              |-- /ws        -> proxy WebSocket to sober-api
              |-- /*         -> serve static SvelteKit assets (SPA fallback)
```

### Asset Serving

Two modes:

- **Embedded (default)** — static assets compiled into the binary via `rust-embed`. Single binary, zero external files. Best for binary distribution.
- **Directory override** — `sober-web --static-dir /path/to/assets/` serves from filesystem instead. Best for development and custom deployments.

### Architecture Position

`sober-web` depends on: `sober-core` (config).
`sober-web` does NOT depend on `sober-api` as a crate — it communicates at runtime via HTTP/WebSocket proxy.

`sober-api` can still be deployed standalone for headless/API-only use cases (bots, programmatic access). `sober-web` is the "full stack" entry point.

### Deployment

| Process | Port/Socket | Public |
|---------|-------------|--------|
| `sober-web` | `:3000` (configurable) | Yes |
| `sober-api` | UDS or `localhost:8080` | No |
| `sober-agent` | UDS | No |
| `sober-scheduler` | UDS | No |

---

## Architecture Impact

### Crate Map Update

Add `sober-web` to the crate map:

| Crate | Responsibility |
|-------|---------------|
| `sober-web` | Static asset serving (embedded + directory), reverse proxy to `sober-api`, SPA routing |

### Dependency Flow Update

```
sober-web ──► sober-core (config only, runtime proxy to sober-api)
```

### Config Additions

| Variable | Purpose |
|----------|---------|
| `MASTER_ENCRYPTION_KEY` | Hex-encoded 256-bit MEK for envelope encryption |
| `WEB_LISTEN_ADDR` | `sober-web` bind address (default `:3000`) |
| `WEB_STATIC_DIR` | Optional override for static asset directory |
| `WEB_API_UPSTREAM` | `sober-api` address to proxy to (UDS path or `http://localhost:8080`) |
