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
│ • Chunk Types│  │ • Envelope Enc │  │ • OpenAI       │
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
| `sober-core` | Shared types, error handling, config, domain primitives. Includes evolution types (`EvolutionEvent`, `EvolutionType`, `EvolutionStatus`, `EvolutionConfigRow`), `EvolutionRepo` trait, and `EvolutionConfig` (interval). |
| `sober-db` | PostgreSQL access layer: pool creation, row types, repository implementations (`Pg*Repo`). Includes `PgEvolutionRepo` for `evolution_events` and `evolution_config` tables. |
| `sober-auth` | Authentication (password, OIDC, passkeys, HW tokens), RBAC/ABAC |
| `sober-memory` | Vector storage, memory pruning, scoped retrieval |
| `sober-agent` | **Binary crate (gRPC server process).** Actor-model agent: one `ConversationActor` per conversation ensures sequential message processing. Write-ahead persistence for tool executions with crash recovery. Real-time LLM streaming. Self-evolution loop: periodic detection job, `propose_*` tools, execution engine, and revert logic. `ExecuteEvolution` / `RevertEvolution` gRPC RPCs. Called by `sober-api` and `sober-scheduler` via gRPC/UDS. Depends on `sober-mind`, `sober-memory`, `sober-crypto`, `sober-llm`, `sober-workspace`, `sober-sandbox`, `sober-plugin-gen`, `sober-skill`. |
| `sober-plugin` | Plugin registry, WASM host functions (13 host functions via Extism), backend service traits, audit pipeline, blob-backed storage |
| `sober-plugin-gen` | Plugin generation pipeline: template scaffolding, WASM compilation, and LLM-powered generation. Depends on `sober-core`, `sober-llm`. |
| `sober-skill` | Skill discovery, loading, and activation. Provides `SkillCatalog`, `SkillLoader`, `ActivateSkillTool`, frontmatter parsing. |
| `sober-crypto` | Keypair management, envelope encryption, signing |
| `sober-api` | HTTP/WebSocket API gateway, rate limiting, channel adapters, Unix admin socket |
| `sober-web` | **Binary crate.** Serves SvelteKit frontend (embedded via `rust-embed` or from disk), reverse-proxies `/api/*` and WebSocket to `sober-api`. |
| `sober-cli` | Unified CLI: config, user management, migrations (offline, direct DB), scheduler control (runtime, via UDS) |
| `sober-mind` | Agent identity (structured instructions + soul.md layering), prompt assembly, visibility filtering, trait evolution, injection detection. Instruction overlay loading for evolution-generated overrides with guardrail blocklist. |
| `sober-scheduler` | Autonomous tick engine, interval + cron scheduling, job persistence, local execution of deterministic jobs (artifact/internal) via executor registry. Depends on `sober-memory`, `sober-sandbox`, `sober-workspace` for local executors. |
| `sober-mcp` | MCP server/client implementation for tool interop. MCP servers run sandboxed via `sober-sandbox`. Depends on `sober-crypto` for credential decryption. |
| `sober-sandbox` | Process-level execution sandboxing (bwrap), policy profiles, network filtering via UDS proxy bridge, audit |
| `sober-llm` | Multi-provider LLM abstraction. Two transports: OpenAI-compatible HTTP (OpenRouter, Ollama, OpenAI, etc.) and ACP (Agent Client Protocol) for sending prompts through local coding agents (Claude Code, Kimi Code, Goose). |
| `sober-workspace` | Workspace business logic: filesystem layout, git operations (git2), blob storage. Used by agent, CLI, and scheduler. |

---

## Memory & Context System

### Scoped Memory

```
System (agent identity, global knowledge)
  └── User Scope (per-user facts, preferences, decisions)
       └── Conversation Scope (session-specific context, auto-loaded per conversation)
```

Three active scopes backed by Qdrant payload filtering (no separate collections):

| Scope | Stored as | Auto-loaded | Searchable via recall |
|-------|-----------|-------------|---------------------|
| **System** | `ScopeId::GLOBAL` | Preferences only | Yes (scope: "system") |
| **User** | `ScopeId::from(user_id)` | Preferences only | Yes (scope: "user", default) |
| **Conversation** | `ScopeId::from(conversation_id)` | All types, current conversation only | Yes (scope: "conversation" + conversation_id) |

The LLM decides scope during memory extraction. Conversation-scoped memories
are invisible to other conversations unless explicitly searched via the `recall`
tool with a conversation_id. Context loading follows principle of least privilege.

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
2. **Soul layers** (per-user/group Qdrant-stored adaptations, appended after soul.md)
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

## Self-Evolution

The agent autonomously proposes improvements by analysing conversation patterns.
All changes are tracked in the `evolution_events` table, configurable per type,
and revertible.

### Evolution Types

| Type | Output | Infrastructure |
|------|--------|---------------|
| **Plugin** | WASM binary tool | `sober-plugin-gen` → `sober-plugin` registry |
| **Skill** | Prompt-based skill file | `sober-skill` catalog reload |
| **Instruction** | Instruction overlay file | `sober-mind` overlay loader (guardrail blocklist enforced) |
| **Automation** | Scheduled job | `sober-scheduler` via gRPC |

### Lifecycle

```
Proposed → Approved → Executing → Active
    │          │           │
    ▼          ▼           ▼
 Rejected    (fail→Failed) Failed
                            │
                         Reverted
```

Status transitions are recorded in `status_history` (JSONB array with timestamps).

### Detection & Proposal Loop

The scheduler triggers a `self_evolution_check` system job on a configurable
interval (default: 2 hours). The job runs as a conversation turn inside the
agent with internal-visibility instructions and four `propose_*` tools:

1. **Gather** — query recent conversations and active evolutions (no LLM tokens).
2. **Detect** — LLM analyses patterns and calls `propose_tool`, `propose_skill`,
   `propose_instruction`, or `propose_automation`.
3. **Auto-approve** — proposals whose type has `autonomy = auto` are approved
   immediately (subject to daily rate limit).
4. **Execute** — approved proposals are executed in the same cycle.

### Execution & Revert

`ExecuteEvolution` and `RevertEvolution` are gRPC RPCs on the agent service.
The API calls them when an admin approves or reverts via the web UI or CLI.

Each type has a dedicated executor and reverter in `sober-agent::evolution`:

| Type | Execute | Revert |
|------|---------|--------|
| Plugin | Generate WASM → register in plugin system | Delete plugin |
| Skill | Generate skill file → register plugin + reload catalog | Delete plugin + skill file, reload |
| Instruction | Write overlay file → reload instructions | Remove overlay (or restore previous), reload |
| Automation | Create scheduled job | Cancel job |

### Autonomy Configuration

Stored in the `evolution_config` singleton table. Each type has an independent
`AutonomyLevel`: `Auto`, `ApprovalRequired`, or `Disabled`. Defaults:

- Plugins / Instructions → `ApprovalRequired` (broader impact)
- Skills / Automations → `Auto` (lower risk, easily reverted)

### Rate Limits

| Limit | Value |
|-------|-------|
| Max proposals per cycle | 5 |
| Max auto-approvals per day | 3 |
| Max concurrent executing | 2 |

### Safety

- **Guardrail blocklist** — instruction evolutions cannot modify files with
  `category: guardrail` frontmatter or files on a hardcoded blocklist.
- **Deduplication** — unique DB index on `(evolution_type, title)` for active
  events, plus tool-level and prompt-level duplicate checks.
- **Audit trail** — every status transition is persisted with timestamp.

---

## Data Storage

| Store | Engine | Purpose |
|-------|--------|---------|
| Primary DB | PostgreSQL 17 | Users, groups, permissions, audit logs, plugin registry, conversation messages, tool executions, workspace settings, evolution events + config |
| Vector Store | Qdrant | Embeddings, similarity search, knowledge retrieval |
| Cache | In-memory (moka) | Route/session caching with PostgreSQL-backed sessions |
| Code Store | Git (libgit2) | Versioned user-generated code, plugin source |
| Search | SearXNG | Meta-search aggregation for web queries |

### Workspace Settings

All workspace-level configuration is stored in the `workspace_settings` table
(one row per workspace). This is the single source of truth for:

- **Permission mode** — controls shell command approval (interactive, policy-based, autonomous).
- **Sandbox policy** — profile name + optional overrides (network mode, allowed domains, timeout, spawn).
- **Snapshot settings** — auto-snapshot flag and max snapshot count.
- **Capability filtering** — blacklist of disabled tools (`TEXT[]`) and disabled plugins (`UUID[]`). Disabled capabilities are silently excluded from the agent's available tools each turn.

Settings are created atomically alongside the workspace via `WorkspaceRepo::provision()`.
The agent loads settings at the start of each turn and uses them to resolve
`SandboxPolicy` for shell executions and filter available tools/plugins.

`.sober/config.toml` no longer controls sandbox, permission, or snapshot settings.

### Sandbox Network Modes

Shell commands run inside bubblewrap (`bwrap`) with one of three network modes:

| Mode | `--unshare-net` | Network access |
|------|-----------------|---------------|
| `None` | Yes | Loopback only — no outbound access |
| `AllowedDomains` | Yes | Only listed domains, via HTTPS CONNECT proxy |
| `Full` | No | Unrestricted host networking |

**AllowedDomains proxy bridge:**

`--unshare-net` isolates the sandbox's network namespace — the process can't
reach the host's loopback. To reach the filtering proxy, a UDS (Unix domain
socket) bridge connects the two namespaces via the filesystem:

```
[sandboxed command]
  → HTTP_PROXY=127.0.0.1:18080  (sandbox loopback)
  → inner socat: TCP-LISTEN:18080 → UNIX-CONNECT:/tmp/sober-proxy-<uuid>.sock
  → bind-mounted UDS socket (crosses namespace boundary)
  → outer socat: UNIX-LISTEN → TCP:127.0.0.1:<proxy-port>
  → HTTP CONNECT proxy (domain allowlist enforcement)
  → internet
```

- **Outer socat** (host): listens on a UDS socket, forwards to the TCP proxy.
- **bwrap**: bind-mounts the UDS socket + socat binary into the sandbox.
- **Inner socat** (sandbox): translates `HTTP_PROXY` TCP traffic to the UDS.
- **Proxy**: Rust async HTTP CONNECT proxy that checks each domain against the
  allowlist. Allowed → tunnel established. Denied → 403 + logged.

Port 18080 is private to each sandbox's network namespace — concurrent
executions don't collide.

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

### Runtime Dependencies

The `sober-agent` image requires these system binaries at runtime:

| Binary | Purpose | Required? |
|--------|---------|-----------|
| `bwrap` (bubblewrap) | Process-level sandboxing | Always |
| `socat` | Network bridge for `AllowedDomains` sandbox mode | When using domain-filtered network |
| `git` | Workspace git operations (libgit2 fallback) | Always |
| `clang`, `lld` | WASM plugin compilation | When generating plugins |

### Docker Image Builds

Two Dockerfile strategies serve different purposes:

| Context | Dockerfiles | Tool | Purpose |
|---------|-------------|------|---------|
| Local dev | `infra/docker/Dockerfile.<service>` (×5) | `docker compose up --build` | Fast single-service iteration |
| CI / prod | `infra/docker/Dockerfile.ci` (×1) | `docker buildx bake -f docker-bake.hcl` | Optimized multi-image publishing |

**CI builds** use a unified multi-stage Dockerfile with [cargo-chef](https://github.com/LukeMathWalker/cargo-chef)
to separate dependency compilation (slow, cacheable) from application code compilation (fast).
All 5 binaries are compiled in a single `cargo build --release`, then each service image copies
its binary into a minimal `debian:trixie-slim` runtime stage. `docker-bake.hcl` defines all 5
targets for `docker buildx bake`, sharing the builder layers across all images.

**Dev builds** keep per-service Dockerfiles for fast iteration — `docker-compose.yml` is unchanged.
