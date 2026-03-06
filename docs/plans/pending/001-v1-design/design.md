# 001 — Sober v1 Design Document

> Full design for the first deployable version of Sober.
> Date: 2026-03-06

---

## 1. Scope & First Milestone

**v1 goal:** A self-hosted AI chat agent with persistent memory, tool use, and a web UI.

A single admin registers users via CLI. Users log in with password, chat with the agent
through a streaming WebSocket connection, and the agent can search the web, fetch URLs,
and call external tools via MCP.

**What v1 includes:**

- 9 Rust crates in a cargo workspace
- SvelteKit frontend (static adapter, Tailwind CSS)
- Password authentication with Argon2id
- Admin-approved user registration via `sober` CLI
- Persistent memory: PostgreSQL + Qdrant + BCF (incremental, no encryption yet)
- Single agent loop with streaming responses
- Tool use: web search (SearXNG), URL fetch, MCP client (stdio)
- Docker Compose deployment on a single VPS
- Caddy for TLS termination, static file serving, reverse proxy

**What v1 explicitly defers:**

- Passkeys, OIDC, magic links
- Email sending
- Plugin/WASM sandboxing (sober-plugin is a stub)
- Replica agents, task delegation
- Discord, WhatsApp, Telegram channels
- `soberctl` (runtime CLI — the admin socket exists but no CLI client yet)
- Sandboxed code execution (plugin/WASM --- bwrap process sandbox is included)
- External API tools beyond MCP
- S3/MinIO blob storage (local filesystem for now)
- BCF encryption and compression (format supports flags, but unused in v1)

---

## 2. Crate Architecture

### 2.1 Crate Map

| Crate | Type | Responsibility |
|-------|------|---------------|
| `sober-core` | lib | Shared types, errors, config, domain primitives, admin protocol types |
| `sober-crypto` | lib | Argon2id hashing, Ed25519 keypairs, injection detection stubs |
| `sober-auth` | lib | Registration, password verification, sessions, RBAC |
| `sober-memory` | lib | BCF format, Qdrant integration, scoped retrieval |
| `sober-llm` | lib | LLM engine trait, Anthropic Claude provider, streaming |
| `sober-mind` | lib | SOUL.md resolution, prompt assembly, access masks, soul layers |
| `sober-agent` | lib | Agent loop, tool trait, v1 tools (web search, URL fetch) |
| `sober-mcp` | lib | MCP client (stdio transport), tool discovery, proxy calls |
| `sober-sandbox` | lib | bwrap process sandbox, policy profiles, network proxy, audit |
| `sober-api` | bin | Axum HTTP/WS server, auth routes, chat, rate limiting, admin socket |
| `sober-cli` | bin | `sober` binary: user management, migrations, config validation |

### 2.2 Dependency Flow

```
sober-api (bin)
  ├── sober-agent
  │     ├── sober-mind
  │     │     ├── sober-memory
  │     │     ├── sober-crypto
  │     │     └── sober-core
  │     ├── sober-sandbox
  │     │     └── sober-core
  │     ├── sober-mcp
  │     │     ├── sober-sandbox
  │     │     └── sober-core
  │     ├── sober-llm
  │     │     └── sober-core
  │     ├── sober-memory
  │     │     └── sober-core
  │     └── sober-core
  ├── sober-auth
  │     ├── sober-crypto
  │     │     └── sober-core
  │     └── sober-core
  └── sober-core

sober-cli (bin)
  └── sober-core
```

**Rules:**

- Dependencies flow downward only. No cycles.
- `sober-api` is a leaf binary — nothing depends on it.
- `sober-cli` depends on `sober-core` only. Admin protocol types live in `sober-core`.
- All crates depend on `sober-core` for shared types and errors.

---

## 3. Data Model

### 3.1 PostgreSQL (relational data)

All tables use UUIDv7 primary keys. All timestamps are `timestamptz`. Every table
with user-owned data has a `scope_id` column for isolation.

Enum-like columns use PostgreSQL custom types (`CREATE TYPE ... AS ENUM`) for
DB-level validation and clean mapping to Rust enums via sqlx. Adding variants
is a one-line `ALTER TYPE ... ADD VALUE` migration.

#### Custom Types

```sql
CREATE TYPE user_status AS ENUM ('pending', 'active', 'disabled');
CREATE TYPE scope_kind AS ENUM ('system', 'user', 'group', 'session');
CREATE TYPE message_role AS ENUM ('user', 'assistant', 'system', 'tool');
```

#### Core Tables

```sql
-- Roles (extensible — new roles via INSERT, not schema change)
CREATE TABLE roles (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,  -- 'user', 'admin', future: 'moderator', etc.
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Seed default roles
INSERT INTO roles (id, name, description) VALUES
    ('00000000-0000-0000-0000-000000000001', 'user', 'Standard user'),
    ('00000000-0000-0000-0000-000000000002', 'admin', 'System administrator');

-- Users
CREATE TABLE users (
    id            UUID PRIMARY KEY,  -- UUIDv7
    email         TEXT NOT NULL UNIQUE,
    username      TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    status        user_status NOT NULL DEFAULT 'pending',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- User-Role mapping (many-to-many, supports multiple roles per user)
CREATE TABLE user_roles (
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id     UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    scope_id    UUID REFERENCES scopes(id),  -- NULL = global, set = scoped to group/context
    granted_by  UUID REFERENCES users(id),
    granted_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, role_id, COALESCE(scope_id, '00000000-0000-0000-0000-000000000000'))
);

-- Sessions
CREATE TABLE sessions (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT NOT NULL UNIQUE,
    expires_at  TIMESTAMPTZ NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Scopes
CREATE TABLE scopes (
    id          UUID PRIMARY KEY,
    kind        scope_kind NOT NULL,
    owner_id    UUID REFERENCES users(id),
    parent_id   UUID REFERENCES scopes(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Conversations
CREATE TABLE conversations (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope_id    UUID NOT NULL REFERENCES scopes(id),
    title       TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Messages
CREATE TABLE messages (
    id              UUID PRIMARY KEY,
    conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    scope_id        UUID NOT NULL REFERENCES scopes(id),
    role            message_role NOT NULL,
    content         TEXT NOT NULL,
    tool_calls      JSONB,         -- structured tool call data if role=assistant
    tool_result     JSONB,         -- tool execution result if role=tool
    token_count     INTEGER,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- MCP Server Configurations (per-user)
CREATE TABLE mcp_servers (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    command     TEXT NOT NULL,
    args        JSONB NOT NULL DEFAULT '[]',
    env         JSONB NOT NULL DEFAULT '{}',
    enabled     BOOLEAN NOT NULL DEFAULT true,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(user_id, name)
);

-- Audit Log
CREATE TABLE audit_log (
    id          UUID PRIMARY KEY,
    actor_id    UUID REFERENCES users(id),
    action      TEXT NOT NULL,
    target_type TEXT,
    target_id   UUID,
    details     JSONB,
    ip_address  INET,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Indexes
CREATE INDEX idx_user_roles_user_id ON user_roles(user_id);
CREATE INDEX idx_user_roles_role_id ON user_roles(role_id);
CREATE INDEX idx_sessions_user_id ON sessions(user_id);
CREATE INDEX idx_sessions_expires_at ON sessions(expires_at);
CREATE INDEX idx_conversations_user_id ON conversations(user_id);
CREATE INDEX idx_messages_conversation_id ON messages(conversation_id);
CREATE INDEX idx_messages_created_at ON messages(created_at);
CREATE INDEX idx_audit_log_actor_id ON audit_log(actor_id);
CREATE INDEX idx_audit_log_created_at ON audit_log(created_at);
```

### 3.2 Qdrant (vector storage)

**Collection naming:** `user_{user_id}` — one collection per user.

**Point schema:**

```json
{
  "id": "<UUIDv7>",
  "vector": [0.0, ...],  // embedding from LLM
  "payload": {
    "scope_id": "<UUID>",
    "chunk_type": "fact|conversation|embedding",
    "content": "...",
    "source_message_id": "<UUID>",
    "importance": 0.85,
    "created_at": "2026-03-06T...",
    "decay_at": "2026-04-06T..."
  }
}
```

**System collection:** `system` — for global knowledge chunks shared across all users.

**Search strategy:** Hybrid search combining dense vectors (cosine similarity) with sparse
BM25 for keyword matching. Qdrant's built-in sparse vector support handles this.

### 3.3 Scope Isolation

```
Global (system scope)
  └── User Scope (per-user — created on registration)
       └── Group Scope (future — shared context for teams)
            └── Session Scope (per-conversation ephemeral context)
```

**Enforcement:**

- Every query includes `scope_id` in its WHERE clause
- Auth middleware resolves the user's permitted scopes
- Qdrant queries filter by `scope_id` in the payload
- A user can never access another user's scope
- Session scopes are created per conversation and pruned on conversation end

### 3.4 Redis

Used for:
- Rate limiting counters (sliding window per user)
- Session token cache (hot path — avoids DB hit on every request)
- Future: hot context cache for frequently accessed memory chunks

---

## 4. Agent Loop & Tool System

### 4.1 Agent Loop

```
User message
  │
  ▼
┌─────────────────────────┐
│ 1. Load context          │ ← Retrieve from Qdrant + recent messages from DB
│    (scoped, minimal)     │
├─────────────────────────┤
│ 2. Build prompt          │ ← sober-mind assembles: SOUL.md + context + history + access mask + tools
├─────────────────────────┤
│ 3. Call LLM (streaming)  │ ← Anthropic Claude, stream tokens to WebSocket
├─────────────────────────┤
│ 4. Parse response        │ ← Check for tool_use blocks
├─────────────────────────┤
│ 5a. If tool call:        │
│     Execute tool          │ ← Run tool, get result
│     Append tool result    │
│     Go to step 2          │ ← Re-enter LLM with tool result
│                           │
│ 5b. If text response:    │
│     Stream to client      │ ← Final response
├─────────────────────────┤
│ 6. Store                 │ ← Save messages to DB, embed + store in Qdrant
└─────────────────────────┘
```

**Max tool iterations:** 10 per user message (prevents runaway loops).

**Context budget:** The agent loads at most N tokens of context (configurable, default
4096) from memory retrieval, plus the last M messages from the conversation (configurable,
default 20).

### 4.2 Tool Trait

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique name for this tool (used in LLM tool definitions).
    fn name(&self) -> &str;

    /// Human-readable description for the LLM.
    fn description(&self) -> &str;

    /// JSON Schema for the tool's input parameters.
    fn input_schema(&self) -> serde_json::Value;

    /// Execute the tool with the given input.
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError>;
}

pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}
```

### 4.3 v1 Tools

| Tool | Description | Implementation |
|------|-------------|---------------|
| `web_search` | Search the web via SearXNG | HTTP GET to local SearXNG instance |
| `fetch_url` | Fetch and extract text from a URL | HTTP GET + HTML-to-text extraction |
| MCP tools | Dynamically discovered from configured MCP servers | `sober-mcp` client, stdio transport |

**SearXNG integration:** SearXNG runs as a Docker container. The agent calls its JSON API
(`/search?q=...&format=json`). No API key needed — it's self-hosted.

**URL fetch:** Uses `reqwest` with timeouts, size limits (1MB), and content-type
filtering (text/html, text/plain, application/json only). HTML is converted to plain
text using a lightweight extractor.

---

## 5. API Surface

### 5.1 REST Endpoints

All under `/api/v1/`. Responses use a consistent envelope:

```json
// Success
{ "data": ... }

// Error
{ "error": { "code": "not_found", "message": "..." } }
```

#### Auth

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/auth/register` | none | Register (creates pending user) |
| POST | `/auth/login` | none | Login, returns session cookie |
| POST | `/auth/logout` | session | Invalidate session |
| GET | `/auth/me` | session | Current user info |

#### Conversations

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/conversations` | session | List user's conversations |
| POST | `/conversations` | session | Create new conversation |
| GET | `/conversations/:id` | session | Get conversation with messages |
| PATCH | `/conversations/:id` | session | Update title |
| DELETE | `/conversations/:id` | session | Delete conversation |

#### MCP Servers

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/mcp/servers` | session | List user's MCP server configs |
| POST | `/mcp/servers` | session | Add MCP server config |
| PATCH | `/mcp/servers/:id` | session | Update MCP server config |
| DELETE | `/mcp/servers/:id` | session | Remove MCP server config |

#### System

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/health` | none | Health check |

### 5.2 WebSocket Protocol

**Endpoint:** `GET /api/v1/ws` (upgrade to WebSocket, session cookie required)

#### Client → Server Messages

```json
// Send a chat message
{
  "type": "chat.message",
  "conversation_id": "<UUID>",
  "content": "Hello, what's the weather?"
}

// Cancel an in-progress response
{
  "type": "chat.cancel",
  "conversation_id": "<UUID>"
}
```

#### Server → Client Messages

```json
// Streaming text chunk
{
  "type": "chat.delta",
  "conversation_id": "<UUID>",
  "message_id": "<UUID>",
  "content": "The weather "
}

// Tool use started
{
  "type": "chat.tool_use",
  "conversation_id": "<UUID>",
  "message_id": "<UUID>",
  "tool_name": "web_search",
  "input": { "query": "weather today" }
}

// Tool result
{
  "type": "chat.tool_result",
  "conversation_id": "<UUID>",
  "message_id": "<UUID>",
  "tool_name": "web_search",
  "output": "..."
}

// Response complete
{
  "type": "chat.done",
  "conversation_id": "<UUID>",
  "message_id": "<UUID>"
}

// Error
{
  "type": "chat.error",
  "conversation_id": "<UUID>",
  "code": "rate_limited",
  "message": "..."
}
```

### 5.3 Admin Socket

Unix domain socket at a configurable path (default `/run/sober/admin.sock`).
Filesystem permissions control access — no authentication over the socket.

The API server listens on this socket when `ADMIN_SOCKET_PATH` is set. The `soberctl`
binary (deferred to post-v1) will connect to it. For v1, the socket exists but only
the health check endpoint is available over it.

**Protocol:** Same HTTP/JSON as the REST API, but over Unix socket.

---

## 6. Frontend

### 6.1 Stack

- SvelteKit with static adapter (output goes to `frontend/build/`)
- Svelte 5 runes only (no legacy patterns)
- Tailwind CSS for styling
- TypeScript strict mode

### 6.2 Pages

| Route | Description |
|-------|-------------|
| `/login` | Login form |
| `/register` | Registration form (shows "pending approval" after submit) |
| `/` | Conversation list (redirects to `/login` if unauthenticated) |
| `/chat/:id` | Chat view with streaming messages |
| `/settings/mcp` | MCP server configuration |

### 6.3 Key Components

- `ChatMessage.svelte` — Renders a single message (user or assistant), handles markdown
- `ChatInput.svelte` — Text input with send button, Shift+Enter for newlines
- `ConversationList.svelte` — Sidebar list of conversations
- `ToolCallDisplay.svelte` — Shows tool invocations inline (collapsible)
- `StreamingText.svelte` — Renders streaming text with cursor

### 6.4 Data Flow

- Auth state loaded in root `+layout.ts` via `GET /api/v1/auth/me`
- Conversation list loaded in `+page.ts` for the index route
- Chat uses WebSocket connection managed in a `.svelte.ts` store
- MCP settings use standard REST calls via `$lib/utils/api.ts`

### 6.5 Static File Serving

In production, Caddy serves the built frontend files directly. The Rust API never
touches static files — Caddy handles `/` and falls back to `index.html` for SPA routing.
API requests (`/api/*`) and WebSocket upgrades (`/api/v1/ws`) are reverse-proxied to
the Rust server.

---

## 7. Deployment

### 7.1 Infrastructure

Single VPS running Docker Compose with these services:

| Service | Image | Port | Purpose |
|---------|-------|------|---------|
| `caddy` | `caddy:2` | 80, 443 | TLS, static files, reverse proxy |
| `sober-api` | custom | 3000 (internal) | Rust API server |
| `postgres` | `postgres:17` | 5432 (internal) | Relational data |
| `qdrant` | `qdrant/qdrant:latest` | 6333, 6334 (internal) | Vector storage |
| `redis` | `redis:7` | 6379 (internal) | Cache, rate limiting |
| `searxng` | `searxng/searxng:latest` | 8080 (internal) | Web search |

### 7.2 Docker Compose Layout

```yaml
# Only Caddy exposes ports to the host
# All other services communicate on an internal network
# Volumes for persistent data: postgres, qdrant, redis, caddy config/certs
```

### 7.3 Caddy Configuration

```
yourdomain.com {
    # Static frontend files
    root * /srv/frontend
    file_server
    try_files {path} /index.html

    # API reverse proxy
    handle /api/* {
        reverse_proxy sober-api:3000
    }

    # WebSocket reverse proxy
    handle /api/v1/ws {
        reverse_proxy sober-api:3000
    }
}
```

Caddy handles automatic HTTPS via Let's Encrypt. No manual certificate management.

### 7.4 Deployment Flow

```
1. docker compose up -d           # Start all services
2. sober migrate run              # Apply database migrations
3. sober user create --admin      # Create first admin user
4. # Users register via web UI → admin approves via CLI
```

---

## 8. Security Model

### 8.1 Authentication

- **Password hashing:** Argon2id with recommended parameters (19 MiB memory, 2 iterations,
  1 parallelism). Parameters stored in the hash string (PHC format).
- **Sessions:** Random 256-bit tokens. SHA-256 hash stored in DB (never the raw token).
  Sent as `HttpOnly`, `Secure`, `SameSite=Lax` cookie. 30-day expiry.
- **Registration:** Users register but start in `pending` status. An admin must approve
  them via `sober user approve <email>` before they can log in.

### 8.2 Authorization (RBAC)

Roles are stored in a `roles` table with a `user_roles` join table, supporting
multiple roles per user and future scoped roles (e.g., group moderator).

v1 seeds two roles:

| Role | Capabilities |
|------|-------------|
| `user` | Chat, manage own conversations, configure own MCP servers |
| `admin` | Everything a user can + approve/disable users, view audit log |

New users get the `user` role on approval. The first user created via
`sober user create --admin` gets both `user` and `admin` roles.

**Scope-aware roles (future):** The `user_roles.scope_id` column is nullable.
`NULL` means the role is global. When set, the role applies only within that
scope (e.g., admin of a specific group). v1 only uses global roles.

All endpoints check roles via session middleware. Scope isolation is enforced at
the query level — users can only access their own data.

### 8.3 Injection Detection

Stub implementation in v1 — logs suspicious patterns but does not block:

- System prompt leakage patterns in user input
- Known prompt injection prefixes
- Unusual Unicode or control characters

Full injection defense (canary tokens, output filtering, lockout) is deferred.

### 8.4 Rate Limiting

Redis-backed sliding window rate limiter:

| Endpoint | Limit |
|----------|-------|
| `POST /auth/login` | 5 per minute per IP |
| `POST /auth/register` | 3 per hour per IP |
| `chat.message` (WS) | 20 per minute per user |
| All other API | 60 per minute per user |

### 8.5 Input Validation

- All request bodies validated with `validator` derive macros
- Email format validation
- Password minimum length (12 characters)
- Content length limits on messages (32 KB)
- MCP server command allow-listing (configurable)

---

## 9. Binary Context Format (BCF) — v1 Subset

v1 implements BCF incrementally — the header and chunk table are the canonical format,
but encryption and compression flags are defined but unused.

### 9.1 Header (16 bytes)

```
Magic:       0x53 0xD5 0x42 0x45 ("SOBE" — Sober with Estonian o)
Version:     1 (u16 LE)
Flags:       0x0000 (u16 — bit 0: encrypted, bit 1: compressed; both 0 in v1)
Scope ID:    first 8 bytes of scope UUID (u64 LE)
Chunk Count: number of chunks (u32 LE)
```

### 9.2 Chunk Table

Array of entries, one per chunk:
```
Offset: u64 LE  — byte offset from start of chunk data section
Length: u32 LE  — byte length of chunk
Type:   u8      — 0=Fact, 1=Conversation, 2=Embedding
```

### 9.3 v1 Chunk Types

| Type | Code | Content |
|------|------|---------|
| Fact | 0 | Extracted factual knowledge, stored as UTF-8 text |
| Conversation | 1 | Conversation summary or key exchange, UTF-8 |
| Embedding | 2 | Raw f32 vector (for offline/backup; primary vectors in Qdrant) |

Skill, Preference, and Code chunk types are reserved (codes 3-5) but not
implemented in v1.

### 9.4 Usage in v1

BCF is used as the **export/backup format** and for **offline context snapshots**.
Live memory operations go through Qdrant (vectors) and PostgreSQL (messages).
BCF files are written when:
- A conversation is archived
- A user exports their data
- The system creates periodic memory snapshots

---

## 10. Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| IDs | UUIDv7 | Time-ordered, sortable, no coordination needed |
| Database | PostgreSQL 17 | Mature, JSONB support, row-level security capable |
| Vector DB | Qdrant | Rust-native client, hybrid search, payload filtering |
| Search | SearXNG | Self-hosted, no API key, meta-search aggregator |
| TLS/Proxy | Caddy | Auto HTTPS, simple config, static file serving |
| Password hash | Argon2id | OWASP recommended, memory-hard, side-channel resistant |
| Crypto backend | aws-lc-rs | Replaces ring, FIPS-capable, actively maintained |
| LLM API format | OpenAI-compatible | De facto standard; works with OpenRouter, Ollama, OpenAI, vLLM, etc. |
| LLM provider (v1) | OpenRouter | Multi-model access (Claude, GPT-4, Llama, etc.) via single API key |
| Frontend framework | SvelteKit (static) | Svelte 5 runes, minimal JS, Caddy serves the build |
| Styling | Tailwind CSS | Utility-first, design tokens via config |
| MCP transport (v1) | stdio | Simplest, works for local MCP servers |
| Session storage | Cookie + DB | HttpOnly cookie, hash in DB, Redis cache for hot path |
| CLI admin | `sober` binary | Direct DB access, no API dependency, works offline |
| Error handling | thiserror + anyhow | thiserror for libs, anyhow in binaries |
| Serialization | serde + bincode | JSON for API, bincode for BCF chunks |
| Compression (future) | zstd | Fast, good ratio, will be used in BCF when enabled |
| Encryption (future) | AES-256-GCM | Will protect BCF chunks when enabled |
| Signing (future) | Ed25519 | For replica authentication when implemented |
| Protobuf | Dropped from v1 | No gRPC needed; JSON REST is sufficient |
| `shared/` directory | Repurposed | Used for shared TypeScript types, not protobuf |

---

## 11. Configuration

All configuration via environment variables. The `sober-core` config module loads
and validates at startup (fail-fast on missing required values).

```env
# Required
DATABASE_URL=postgres://sober:password@localhost:5432/sober
QDRANT_URL=http://localhost:6334
REDIS_URL=redis://localhost:6379
LLM_BASE_URL=https://openrouter.ai/api/v1
LLM_API_KEY=sk-or-...
LLM_MODEL=anthropic/claude-sonnet-4

# Optional (with defaults)
HOST=0.0.0.0
PORT=3000
LOG_LEVEL=info                          # trace|debug|info|warn|error
LOG_FORMAT=pretty                       # pretty|json
SESSION_TTL_DAYS=30
SEARXNG_URL=http://localhost:8080
ADMIN_SOCKET_PATH=/run/sober/admin.sock # set to enable admin socket
RATE_LIMIT_ENABLED=true
MAX_TOOL_ITERATIONS=10
CONTEXT_TOKEN_BUDGET=4096
CONVERSATION_HISTORY_LIMIT=20
```

---

## 12. Tracing & Observability

- `tracing` crate with `tracing-subscriber`
- Dev: `fmt` layer with `pretty` format, `DEBUG` level
- Prod: `fmt` layer with `json` format, `INFO` level
- Structured fields on all spans: `user_id`, `conversation_id`, `request_id`
- Request tracing via `tower-http::trace`
- Database query timing via `sqlx` built-in tracing

OpenTelemetry export is deferred to post-v1.

---

## 13. Testing Strategy

| Level | Tool | Scope |
|-------|------|-------|
| Unit | `cargo test` | Each crate's `src/` modules |
| Integration | `cargo test` | `tests/` directories, requires running services |
| Property | `proptest` | Crypto operations (Argon2, Ed25519) |
| API | `tower::ServiceExt::oneshot` | Route handlers without a running server |
| Frontend | `vitest` + `@testing-library/svelte` | Component tests |
| E2E | Playwright (deferred) | Full flow tests (post-v1) |

CI runs: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace`,
`cargo audit`, `pnpm check`, `pnpm test`.
