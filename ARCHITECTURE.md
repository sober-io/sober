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
│  PWA (Svelte)  │  Discord Bot  │  WhatsApp  │  CLI  │  API │
└──────┬──────────────┬──────────────┬──────────┬──────┬──────┘
       │              │              │          │      │
       ▼              ▼              ▼          ▼      ▼
┌─────────────────────────────────────────────────────────────┐
│                    API Gateway (sober-api)                   │
│  Rate Limiting │ Auth Middleware │ Channel Routing │ WAF     │
└──────────────────────────┬──────────────────────────────────┘
                           │
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
                          │
       ┌──────────────────┼──────────────────┐
       ▼                  ▼                  ▼
┌──────────────┐  ┌────────────────┐  ┌────────────────┐
│ sober-memory │  │  sober-crypto  │  │   sober-llm    │
│              │  │                │  │                │
│ • Vector DB  │  │ • Keypair Gen  │  │ • Anthropic    │
│ • Binary Ctx │  │ • Envelope Enc │  │ • OpenAI       │
│ • Pruning    │  │ • Signing      │  │ • Local/Ollama │
│ • Scoping    │  │ • Injection    │  │ • Router       │
│              │  │   Detection    │  │                │
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
| `sober-auth` | Authentication (password, OIDC, passkeys, HW tokens), RBAC/ABAC |
| `sober-memory` | Vector storage, binary context format, pruning, scoped retrieval |
| `sober-agent` | Agent orchestration, replica lifecycle, task delegation, self-evolution |
| `sober-plugin` | Plugin registry, sandboxed execution, security audit, code generation |
| `sober-crypto` | Keypair management, envelope encryption, signing, injection detection |
| `sober-api` | HTTP/WebSocket API gateway, rate limiting, channel adapters |
| `sober-mcp` | MCP server/client implementation for tool interop |
| `sober-llm` | Multi-provider LLM abstraction (Anthropic, OpenAI, Ollama, etc.) |

---

## Memory & Context System

### Binary Context Format (BCF)

Replaces naive markdown-based memory with a compact binary format:

```
┌─────────────────────────────────────┐
│ BCF Header (16 bytes)               │
│  Magic: 0x53 0xD5 0x42 0x45 (SÕBE) │
│  Version: u16                       │
│  Flags: u16 (encrypted, compressed) │
│  Scope ID: u64                      │
│  Chunk Count: u32                   │
├─────────────────────────────────────┤
│ Chunk Table (variable)              │
│  [offset: u64, len: u32, type: u8]  │
├─────────────────────────────────────┤
│ Chunks (variable)                   │
│  Each: zstd-compressed, then        │
│  optionally AES-256-GCM encrypted   │
├─────────────────────────────────────┤
│ Vector Index Footer                 │
│  Embedded HNSW index for fast       │
│  similarity search within context   │
└─────────────────────────────────────┘
```

Chunk types: `Fact`, `Conversation`, `Skill`, `Preference`, `Embedding`, `Code`.

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

## Agent & Replica System

### Replica Lifecycle

```
1. SPAWN    → Generate Ed25519 keypair, register public key with parent
2. HANDSHAKE → Mutual TLS + signed challenge-response
3. DELEGATE → Parent sends encrypted task envelope
4. EXECUTE  → Replica loads minimal context, runs in sandbox
5. REPORT   → Signed result returned to parent
6. PRUNE    → Replica scrubs local context, retains only signed audit log
```

### Task Delegation Protocol

```rust
struct TaskEnvelope {
    task_id: Uuid,
    parent_signature: Ed25519Signature,
    encrypted_payload: AES256GCMCiphertext,  // task + minimal context
    scope_grants: Vec<ScopeGrant>,           // what the replica may access
    deadline: Option<DateTime<Utc>>,
    priority: TaskPriority,
}
```

Only the parent agent can issue commands to its replicas. Replicas cannot
command other replicas unless explicitly delegated that authority.

---

## Security Model

### Prompt Injection Defense

1. **Input Sanitization** — All user input passes through injection classifier
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

```rust
enum Permission {
    // Knowledge
    ReadKnowledge(ScopeId),
    WriteKnowledge(ScopeId),
    // Tools
    ExecuteTool(ToolId),
    InstallPlugin,
    // Agent
    SpawnReplica,
    DelegateTask,
    // Admin
    ManageUsers,
    ManageGroups,
    AuditLogs,
}
```

Permissions are scoped — a user may have `ReadKnowledge` for their own scope
but not for another user's scope. Group admins can grant group-scoped permissions.

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

### Plugin Interface (MCP-Compatible)

```rust
#[async_trait]
trait SoberPlugin: Send + Sync {
    fn metadata(&self) -> PluginMetadata;
    fn capabilities(&self) -> Vec<Capability>;
    async fn execute(&self, ctx: &SandboxContext, input: ToolInput) -> Result<ToolOutput>;
    fn audit_report(&self) -> AuditReport;
}
```

---

## LLM Engine Abstraction

```rust
#[async_trait]
trait LlmEngine: Send + Sync {
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse>;
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    fn capabilities(&self) -> EngineCapabilities;
    fn model_id(&self) -> &str;
}
```

Supported backends: Anthropic (Claude), OpenAI, Ollama (local), and any
OpenAI-compatible API. Router selects engine based on task type, cost,
latency requirements, and user preferences.

---

## Communication Channels

### Phase 1: PWA (Svelte)
- SvelteKit with SSR
- WebSocket for real-time agent communication
- Service Worker for offline capability
- Push notifications

### Phase 2+: Additional Channels
- Discord bot (via gateway API)
- WhatsApp (via Business API)
- Telegram
- CLI tool
- Native mobile (Tauri)

All channels route through the unified API gateway with channel-specific
adapters that normalize messages into internal `AgentMessage` format.

---

## Data Storage

| Store | Engine | Purpose |
|-------|--------|---------|
| Primary DB | PostgreSQL 16 | Users, groups, permissions, audit logs, plugin registry |
| Vector Store | Qdrant | Embeddings, similarity search, knowledge retrieval |
| Blob Store | S3-compatible (MinIO) | Large artifacts, code snapshots, binary contexts |
| Cache | Redis | Session tokens, rate limiting, hot context cache |
| Code Store | Git (libgit2) | Versioned user-generated code, plugin source |

---

## Deployment

```
Docker Compose (dev) → Kubernetes (prod)
```

Each crate compiles to a separate binary where appropriate (API gateway,
agent worker, etc.) or is linked as a library crate.
