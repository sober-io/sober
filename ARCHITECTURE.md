# Sõber Architecture

> *Sõber* — "friend" in Estonian. Your best, securest, and most scalable personal AI agent assistant.

## Vision

A self-evolving multi-agent system where a primary agent ("Sõber") can spawn, command, and
orchestrate replica agents across distributed systems. Each replica is cryptographically bound
to its parent, operates in isolated contexts, and can be delegated work autonomously.

---

## Core Principles

1. **Security First** — Zero trust. Every boundary is authenticated and encrypted.
2. **Context Isolation** — User, group, and system contexts never leak across boundaries.
3. **Minimal Context Loading** — Load only what's needed; aggressively offload to external memory.
4. **Self-Evolution** — The system improves itself through audited plugin/skill installation.
5. **Source of Truth** — Executable code is always stored as versioned source; binaries are ephemeral.

---

## System Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Clients                              │
│  PWA (Svelte)  │  Discord Bot  │  WhatsApp  │  CLI (sober/soberctl)  │  API │
└──────┬──────────────┬──────────────┬──────────┬──────┬──────┘
       │              │              │          │      │
       ▼              ▼              ▼          ▼      ▼
┌─────────────────────────────────────────────────────────────┐
│                    sober-web (reverse proxy)                 │
│  Embedded static files │ SPA fallback │ API/WS proxy        │
└──────────────────────────┬──────────────────────────────────┘
                           │
┌─────────────────────────────────────────────────────────────┐
│                    API Gateway (sober-api)                   │
│  Rate Limiting │ Auth Middleware │ Channel Routing │ Admin Socket │
└──────────────────────────┬──────────────────────────────────┘
                           │ gRPC/UDS
       ┌───────────────────┼───────────────────┐
       ▼                   ▼                   ▼
┌──────────────┐  ┌────────────────┐  ┌────────────────┐
│  sober-auth  │  │  sober-agent   │  │  sober-plugin  │
│              │  │                │  │                │
│ • Password   │  │ • Orchestrator │  │ • Registry     │
│ • OIDC       │  │ • Replica Mgmt │  │ • Sandbox      │
│ • Passkeys   │  │ • Task Queue   │  │ • Audit Engine │
│ • HW Tokens  │  │ • Delegation   │  │ • Code Gen     │
│ • RBAC/ABAC  │  │                │  │                │
└──────────────┘  └───────┬────────┘  └────────────────┘
                          │ gRPC/UDS
                          ▲
                          │
                 ┌────────┴────────┐
                 │ sober-scheduler │
                 │                 │
                 │ • Tick Engine   │
                 │ • Cron + Interval│
                 │ • Job Persist   │
                 │ • Admin Socket  │
                 └─────────────────┘
                          │
       ┌──────────────────┼──────────────────┐
       ▼                  ▼                  ▼
┌──────────────┐  ┌────────────────┐  ┌────────────────┐
│ sober-memory │  │  sober-crypto  │  │   sober-llm    │
│              │  │                │  │                │
│ • Vector DB  │  │ • Keypair Gen  │  │ • Anthropic    │
│ • Binary Ctx │  │ • Envelope Enc │  │ • OpenAI       │
│ • Pruning    │  │ • Signing      │  │ • Local/Ollama │
│ • Scoping    │  │                │  │ • Router       │
│              │  │                │  │                │
└──────────────┘  └────────────────┘  └────────────────┘
       │
       ▼
┌─────────────────────────────────────────────────────────────┐
│                     Storage Layer                            │
│  PostgreSQL (relational)  │  Qdrant (vectors)  │  S3 (blob) │
└─────────────────────────────────────────────────────────────┘
```

---

## Crate Map

| Crate | Responsibility |
|-------|---------------|
| `sober-core` | Shared types, error handling, config, domain primitives |
| `sober-db` | PostgreSQL access layer: pool creation, row types, repository implementations (`Pg*Repo`) |
| `sober-auth` | Authentication (password, OIDC, passkeys, HW tokens), RBAC/ABAC |
| `sober-memory` | Vector storage, binary context format, pruning, scoped retrieval |
| `sober-agent` | **Binary crate (gRPC server process).** Agent orchestration, replica lifecycle, task delegation, self-evolution. Called by `sober-api` and `sober-scheduler` via gRPC/UDS. |
| `sober-plugin` | Plugin registry, sandboxed execution, security audit, code generation |
| `sober-crypto` | Keypair management, envelope encryption, signing |
| `sober-api` | HTTP/WebSocket API gateway, rate limiting, channel adapters, Unix admin socket |
| `sober-web` | **Binary crate.** Serves SvelteKit frontend (embedded via `rust-embed` or from disk), reverse-proxies `/api/*` and WebSocket to `sober-api`. |
| `sober-cli` | CLI administration: `sober` (offline DB/migration ops) + `soberctl` (runtime agent/system ops via Unix socket) |
| `sober-mind` | Agent identity (SOUL.md), prompt assembly, access masks, trait evolution, injection detection |
| `sober-scheduler` | Autonomous tick engine, interval + cron scheduling, job persistence, local execution of deterministic jobs (artifact/internal) via executor registry. Depends on `sober-memory`, `sober-sandbox`, `sober-workspace` for local executors. |
| `sober-mcp` | MCP server/client implementation for tool interop. MCP servers run sandboxed via `sober-sandbox`. |
| `sober-sandbox` | Process-level execution sandboxing (bwrap), policy profiles, network filtering, audit |
| `sober-llm` | Multi-provider LLM abstraction. Two transports: OpenAI-compatible HTTP (OpenRouter, Ollama, OpenAI, etc.) and ACP (Agent Client Protocol) for sending prompts through local coding agents (Claude Code, Kimi Code, Goose). |
| `sober-workspace` | Workspace business logic: filesystem layout, git operations (git2), blob storage, config parsing. Used by agent, CLI, and scheduler. |

---

## Memory & Context System

### Binary Context Format (BCF)

Compact binary format: 28-byte header (magic `SÕBE`, version, flags, scope UUID,
chunk count) → chunk table → zstd-compressed + optionally AES-256-GCM encrypted
chunks → embedded HNSW vector index footer.

Chunk types: `Fact`, `Conversation`, `Skill`, `Preference`, `Embedding`, `Code`, `Soul`.

### Scoped Memory

```
Global (system prompts, core personality)
  └── User Scope (per-user facts, preferences, history)
       └── Group Scope (shared context for teams/channels)
            └── Session Scope (ephemeral, current conversation)
```

Each scope is a separate BCF container. Context loading follows principle of
least privilege — only the minimal required scopes are loaded for any operation.

### Vector Storage (Qdrant)

- All knowledge chunks are embedded and indexed
- Scoped collections: `user_{id}`, `group_{id}`, `system`
- Aggressive TTL-based pruning with importance scoring
- Hybrid search: dense vectors + sparse BM25 for keyword matching

---

## Internal Service Communication

### Protocol: gRPC over Unix Domain Sockets

All inter-service communication uses gRPC (tonic + prost) over Unix domain sockets.
Proto definitions live in `backend/proto/`. This avoids circular crate dependencies —
services generate client/server code from shared proto files and communicate at runtime.

### Event Delivery: SubscribeConversationUpdates

Conversation events (text deltas, tool calls, new messages, title changes) are
delivered through a **subscription model** that decouples event production from
the caller:

```
API ──SubscribeConversationUpdates──▶ Agent
                                        │
     ◀── stream of ConversationUpdate ──┘
```

- `HandleMessage` is a **unary RPC** — accepts a user message, returns an ack
  with the stored message ID. The agent processes asynchronously.
- The agent publishes all conversation events to an internal broadcast channel.
- `SubscribeConversationUpdates` is a **server-streaming RPC** — the API calls
  it once on startup and receives events for all conversations.
- The API routes events to the correct WebSocket(s) via a `ConnectionRegistry`
  keyed by `conversation_id`.

This means any trigger (user via WebSocket, scheduler job, future channels)
produces events that reach the frontend without the caller needing to relay them.

`ConversationUpdate` carries a typed `oneof event`: `NewMessage`, `TitleChanged`,
`TextDelta`, `ToolCallStart`, `ToolCallResult`, `ThinkingDelta`, `ConfirmRequest`,
`Done`, `Error`.

### Security

**Filesystem permissions** — Socket files owned by `sober:sober` with `0660`
permissions. Only processes running as the right user can connect. All services
run on the same machine in a trusted network.

For distributed deployment, upgrade to mTLS at the transport layer.

---

## Scheduler

Independent tick engine peer to `sober-api`. Supports interval-based (`every: 30s`)
and cron (`"0 9 * * MON-FRI"`) scheduling. Jobs are persistent (PostgreSQL).
Managed via `soberctl` or agent gRPC calls.

Jobs are routed by payload type:
- **Prompt** → dispatched to `sober-agent` via gRPC (LLM pipeline)
- **Internal** → executed locally via `JobExecutorRegistry` (memory pruning, session cleanup)
- **Artifact** → blob resolved from `sober-workspace`, run in `sober-sandbox`

After local execution, the scheduler notifies the agent via `WakeAgent` RPC.
Prompt job results are delivered to conversations via the `SubscribeConversationUpdates`
stream — the API receives them and pushes to the user's WebSocket.

---

## Security Model

### Prompt Injection Defense

1. **Input Sanitization** — All user input passes through injection classifier (owned by `sober-mind`)
2. **Canary Tokens** — Hidden markers in system prompts detect leakage
3. **Output Filtering** — Responses scanned for leaked system context
4. **Lockout** — Detected injection attempts trigger actor lockout + alert
5. **Context Firewall** — System/private context stored in separate memory
   regions, never concatenated raw with user input

### Authentication Stack

| Method | Use Case |
|--------|----------|
| Password + Argon2id | Primary local auth |
| OIDC (Google, GitHub, etc.) | Federated identity |
| WebAuthn/Passkeys | Passwordless primary |
| FIDO2 Hardware Tokens | High-security access |
| API Keys (HMAC-signed) | Programmatic access |

### Authorization: RBAC + ABAC Hybrid

Permissions are scoped (knowledge, tools, agent, admin). A user may have
`ReadKnowledge` for their own scope but not another's. Group admins can grant
group-scoped permissions.

---

## Agent Mind — Identity & Prompt Assembly

### SOUL.md Resolution Chain

The agent's identity is defined by a layered SOUL.md system:

```
backend/soul/SOUL.md           (base — shipped with the system)
  └── ~/.sober/SOUL.md          (user-level overrides/extensions)
       └── ./.sober/SOUL.md     (workspace/project-level)
```

| Layer | Override rules |
|-------|---------------|
| Base | Foundation — defines everything |
| User (`~/.sober/`) | Full override of base. User controls their instance. |
| Workspace (`./.sober/`) | Additive only. Can override style/domain. Cannot contradict ethical boundaries or security rules. |

### Dynamic Prompt Assembly

No hardcoded prompt tiers. One engine composes the system prompt from:

1. **Resolved SOUL.md** (base + user + workspace layers)
2. **Soul layers** (per-user/group BCF adaptations)
3. **Task context** (what triggered this interaction)
4. **Access mask** (what the caller can see and do)
5. **Relevant memory** (scoped BCF retrieval)

Access masks vary by trigger:

| Trigger | Access |
|---------|--------|
| Scheduler / internal | Full — self-reasoning, memory modification, code proposals |
| Human interaction | Restricted — no internal state, deep requests forwarded to internal tier |
| Replica delegation | Scoped — only what the delegation grants |
| Admin (soberctl) | Full read, restricted write |

### Trait Evolution

Per-user/group soul layers evolve autonomously. Base SOUL.md changes require
high confidence (consistent pattern across many contexts) or admin approval.
All proposed changes are audit-logged.

### Self-Modification Scope

| Target | Autonomy |
|--------|----------|
| Memory / soul layers | Free — autonomous |
| Plugins / skills | Autonomous with sandbox testing + audit pipeline |
| Base SOUL.md | High confidence auto-adopt OR admin approval |
| Core crate code | Propose only — diff + reasoning + tests queued for admin |

---

## Plugin System

### Lifecycle

```
DISCOVER → AUDIT → SANDBOX_TEST → INSTALL → MONITOR → UPDATE/REMOVE
```

### Security Audit Pipeline

1. **Static Analysis** — AST scanning for dangerous patterns
2. **Capability Declaration** — Plugins must declare all required permissions
3. **Sandbox Execution** — First run in WASM sandbox (wasmtime)
4. **Behavioral Analysis** — Monitor syscalls, network access, memory usage
5. **Code Generation** — For predictable plugin logic, generate native Rust/WASM
   that can execute without LLM in the loop

Plugins implement the `SoberPlugin` trait (metadata, capabilities, sandboxed execute, audit report). MCP-compatible.

---

## LLM Engine Abstraction

`LlmEngine` trait with `complete()`, `embed()`, `capabilities()`, `model_id()`.

Two transports: `OpenAiCompatibleEngine` (HTTP to OpenRouter/OpenAI/Ollama) and
`AcpEngine` (JSON-RPC/stdio to local coding agents like Claude Code via
[Agent Client Protocol](https://agentclientprotocol.com/)).

Router selects engine based on task type, cost, latency, and user preferences.

---

## Communication Channels

SvelteKit PWA with WebSocket for real-time agent communication.
All channels route through the API gateway. The WebSocket connection
multiplexes conversations on a single socket. Events arrive via the
agent subscription stream (`SubscribeConversationUpdates`) and are
routed to the correct WebSocket by `conversation_id`.

---

## Data Storage

| Store | Engine | Purpose |
|-------|--------|---------|
| Primary DB | PostgreSQL 17 | Users, groups, permissions, audit logs, plugin registry |
| Vector Store | Qdrant | Embeddings, similarity search, knowledge retrieval |
| Blob Store | S3-compatible (MinIO) | Large artifacts, code snapshots, binary contexts |
| Cache | In-memory (moka) / Redis | v1: in-memory cache (moka) with PostgreSQL-backed sessions. Redis added when horizontal scaling requires shared cache across instances. |
| Code Store | Git (libgit2) | Versioned user-generated code, plugin source |

---

## Deployment

Docker Compose (dev) → Kubernetes (prod). Four independent processes:

| Process | Role | Socket / Port |
|---------|------|---------------|
| `sober-web` | Reverse proxy + static frontend | `:8080` (HTTP) |
| `sober-api` | HTTP/WS gateway, user-driven entry point | `/run/sober/api-admin.sock` |
| `sober-scheduler` | Autonomous tick engine, time-driven entry point | `/run/sober/scheduler.sock` |
| `sober-agent` | gRPC server, invoked by both API and scheduler | `/run/sober/agent.sock` |

Each process can be started, stopped, and scaled independently.

### CLI Administration

`sober` (offline, direct PostgreSQL) and `soberctl` (runtime, via Unix admin sockets).
Admin sockets secured by filesystem permissions (`0660`, `sober:sober`).
