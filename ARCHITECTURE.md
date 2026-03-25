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
│  PWA (Svelte)  │  Discord Bot  │  WhatsApp  │  CLI (sober)  │  API │
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
│ • Password   │  │ • Actor Model  │  │ • Registry     │
│ • OIDC       │  │ • Orchestrator │  │ • Sandbox      │
│ • Passkeys   │  │ • Write-ahead  │  │ • Audit Engine │
│ • HW Tokens  │  │ • Streaming    │  │ • Code Gen     │
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
| `sober-agent` | **Binary crate (gRPC server process).** Actor-model agent: one `ConversationActor` per conversation ensures sequential message processing. Write-ahead persistence for tool executions with crash recovery. Real-time LLM streaming. Called by `sober-api` and `sober-scheduler` via gRPC/UDS. Depends on `sober-mind`, `sober-memory`, `sober-crypto`, `sober-llm`, `sober-workspace`, `sober-sandbox`, `sober-plugin-gen`, `sober-skill`. |
| `sober-plugin` | Plugin registry, WASM host functions (13 host functions via Extism), backend service traits, audit pipeline, blob-backed storage |
| `sober-plugin-gen` | Plugin generation pipeline: template scaffolding, WASM compilation, and LLM-powered generation. Depends on `sober-core`, `sober-llm`. |
| `sober-skill` | Skill discovery, loading, and activation. Provides `SkillCatalog`, `SkillLoader`, `ActivateSkillTool`, frontmatter parsing. |
| `sober-crypto` | Keypair management, envelope encryption, signing |
| `sober-api` | HTTP/WebSocket API gateway, rate limiting, channel adapters, Unix admin socket |
| `sober-web` | **Binary crate.** Serves SvelteKit frontend (embedded via `rust-embed` or from disk), reverse-proxies `/api/*` and WebSocket to `sober-api`. |
| `sober-cli` | Unified CLI: config, user management, migrations (offline, direct DB), scheduler control (runtime, via UDS) |
| `sober-mind` | Agent identity (structured instructions + soul.md layering), prompt assembly, visibility filtering, trait evolution, injection detection |
| `sober-scheduler` | Autonomous tick engine, interval + cron scheduling, job persistence, local execution of deterministic jobs (artifact/internal) via executor registry. Depends on `sober-memory`, `sober-sandbox`, `sober-workspace` for local executors. |
| `sober-mcp` | MCP server/client implementation for tool interop. MCP servers run sandboxed via `sober-sandbox`. Depends on `sober-crypto` for credential decryption. |
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

## Event Delivery: SubscribeConversationUpdates

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
`TextDelta`, `ToolExecutionUpdate`, `ThinkingDelta`, `ConfirmRequest`,
`Done`, `Error`.

### Scheduler Job Routing

Jobs are routed by payload type:
- **Prompt** → dispatched to `sober-agent` via gRPC (LLM pipeline)
- **Internal** → executed locally via `JobExecutorRegistry` (memory pruning, session cleanup)
- **Artifact** → blob resolved from `sober-workspace`, run in `sober-sandbox`

After local execution, the scheduler notifies the agent via `WakeAgent` RPC.

---

## Agent Processing Model

### Actor Model

Each conversation gets a long-lived `ConversationActor` (tokio task) that processes
messages sequentially through an inbox channel. An `ActorRegistry` (`DashMap`)
maps `ConversationId` → `ActorHandle` (sender half of the inbox channel).

```
HandleMessage RPC
    → Agent::handle_message()
        → ActorRegistry.get_or_spawn(conversation_id)
            → inbox_tx.send(InboxMessage::UserMessage { ... })

ConversationActor (one per conversation):
    loop {
        recv from inbox (5 min idle timeout)
        → handle_message_inner()
        → run_turn() → LLM stream → dispatch tool calls → loop
    }
```

**Key invariant:** One actor per conversation, one message at a time. No concurrent
processing races within a conversation.

### Write-Ahead Tool Execution Persistence

Tool executions are tracked in a dedicated `conversation_tool_executions` table
with FK to the assistant message that triggered them. The dispatch pipeline:

1. LLM returns assistant message + `tool_calls`
2. Store assistant message → `conversation_messages`
3. For each tool call:
   - `INSERT` with `status='pending'` (write-ahead)
   - `UPDATE` to `'running'`
   - Execute tool
   - `UPDATE` to `'completed'` or `'failed'` with output/error

`ToolExecutionUpdate` events stream each status transition to the frontend in
real-time. The unique constraint `(conversation_message_id, tool_call_id)` makes
orphaned tool calls structurally impossible.

### Crash Recovery

On actor startup, `recover_incomplete_executions()` queries for any tool executions
still in `pending` or `running` status and marks them `failed` with
`"Agent restarted during execution"`. This ensures no stale in-progress state
persists across restarts.

### LLM Streaming

The agent streams LLM responses token-by-token via `llm.stream()`. `TextDelta`
events are forwarded immediately to the frontend. Tool call fragments are buffered
until the stream completes, then assembled and dispatched.

---

## Security Model

### Prompt Injection Defense (design intent)

1. **Input Sanitization** — All user input passes through injection classifier (owned by `sober-mind`)
2. **Canary Tokens** — Hidden markers in system prompts detect leakage
3. **Output Filtering** — Responses scanned for leaked system context
4. **Lockout** — Detected injection attempts trigger actor lockout + alert
5. **Context Firewall** — System/private context stored in separate memory
   regions, never concatenated raw with user input

### Authentication Stack

Password (Argon2id), OIDC, WebAuthn/Passkeys, FIDO2 hardware tokens, HMAC-signed API keys.

### Authorization: RBAC + ABAC Hybrid

Permissions are scoped (knowledge, tools, agent, admin). A user may have
`ReadKnowledge` for their own scope but not another's. Group admins can grant
group-scoped permissions.

---

## Agent Mind — Identity & Prompt Assembly

### Structured Instruction Directory

Base instructions live in `backend/crates/sober-mind/instructions/*.md`, each
with YAML frontmatter (`category`, `visibility`, `priority`). They are compiled
into the binary via `include_str!()` — zero runtime I/O for base instructions.

| Category (assembly order) | Description |
|---------------------------|-------------|
| `personality` | Identity, values, communication style (`soul.md`) |
| `guardrail` | Ethics, security rules, safety (`safety.md`) |
| `behavior` | Memory, reasoning, evolution |
| `operation` | Tool use, workspace, artifacts, extraction |

Visibility: `public` (all triggers) or `internal` (Admin + Scheduler only).

### soul.md Resolution Chain

The agent's personality (`soul.md`) is layered:

```
sober-mind/instructions/soul.md  (base — compiled into binary)
  └── ~/.sober/soul.md            (user-level override)
       └── ./.sober/soul.md       (workspace/project-level, additive)
```

| Layer | Override rules |
|-------|---------------|
| Base | Foundation — defines everything |
| User (`~/.sober/`) | Full override of base. User controls their instance. |
| Workspace (`./.sober/`) | Additive only. Can override style/domain. Cannot contradict ethical boundaries or security rules. |

### Dynamic Prompt Assembly

One engine composes the system prompt from:

1. **Resolved soul.md** (base + user + workspace layering via `SoulResolver`)
2. **Soul layers** (per-user/group BCF adaptations, appended after soul.md)
3. **Instruction files** (filtered by visibility, sorted by category/priority)
4. **Task context** (what triggered this interaction)
5. **Tool definitions** (available tools for the current turn)

Visibility filtering by trigger:

| Trigger | public | internal |
|---------|--------|----------|
| Human | yes | no |
| Replica | yes | no |
| Admin | yes | yes |
| Scheduler | yes | yes |

### Trait Evolution

Per-user/group soul layers evolve autonomously. Base soul.md changes require
high confidence (consistent pattern across many contexts) or admin approval.
All proposed changes are audit-logged.

### Self-Modification Scope

| Target | Autonomy |
|--------|----------|
| Memory / soul layers | Free — autonomous |
| Plugins / skills | Autonomous with sandbox testing + audit pipeline |
| Base soul.md | High confidence auto-adopt OR admin approval |
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
3. **Sandbox Execution** — First run in WASM sandbox (Extism, wasmtime-backed)
4. **Behavioral Analysis** — Monitor syscalls, network access, memory usage
5. **Code Generation** — For predictable plugin logic, generate native Rust/WASM
   that can execute without LLM in the loop

Plugins declare capabilities in a TOML manifest (`plugin.toml`) and export tool functions via `#[plugin_fn]`. The host wires 11 capability-gated host functions (KV, HTTP, secrets, LLM, memory, conversation, scheduling, filesystem, metrics, tool calls) through Extism's `UserData` mechanism. Generated WASM binaries are stored content-addressed in `BlobStore`.

---

## Data Storage

| Store | Engine | Purpose |
|-------|--------|---------|
| Primary DB | PostgreSQL 17 | Users, groups, permissions, audit logs, plugin registry, conversation messages, tool executions |
| Vector Store | Qdrant | Embeddings, similarity search, knowledge retrieval |
| Cache | In-memory (moka) | Route/session caching with PostgreSQL-backed sessions |
| Code Store | Git (libgit2) | Versioned user-generated code, plugin source |
| Search | SearXNG | Meta-search aggregation for web queries |

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
