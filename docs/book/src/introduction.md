# Introduction

## What is Sõber?

Sõber (Estonian for "friend") is a self-evolving, security-first AI agent system designed for
individuals and teams who want a personal AI assistant that grows with them — without
compromising privacy or control.

At its core, Sõber is a multi-agent platform where a primary agent can spawn, command, and
orchestrate replica agents across distributed systems. Each replica is cryptographically bound
to its parent, operates in fully isolated contexts, and can be delegated work autonomously.
Sõber is built to run on infrastructure you control, with your data staying where you put it.

---

## Key Concepts

### Agents and Replicas

The **primary agent** is your main Sõber instance. It maintains your identity, preferences, and
long-term memory. When given complex or parallelisable tasks, the primary agent spawns
**replica agents** — short-lived workers that inherit a scoped view of context from their parent,
execute a delegated task, and report results back.

Replicas are cryptographically bound to the parent that created them: each one carries a signed
delegation token that authorises its scope of work. A rogue or compromised replica cannot
escalate beyond the permissions it was granted.

### Plugins and Skills

Sõber extends its own capabilities through a **plugin system**. Plugins are WebAssembly modules
that run in a sandboxed environment (backed by Extism/wasmtime). The agent can discover, audit,
install, and monitor plugins autonomously — but only after passing a multi-stage security
pipeline:

1. Static analysis of the plugin's source or bytecode
2. Capability declaration review (plugins must declare every permission they need)
3. Sandboxed test execution with behavioural monitoring
4. Continuous monitoring post-install

Plugins declare their capabilities in a `plugin.toml` manifest and interact with the host
through a set of gated host functions: key-value storage, HTTP requests, secret access, LLM
calls, memory read/write, conversation events, scheduling, filesystem access, metrics, and
tool invocations.

### Memory Scopes

Sõber organises knowledge into **scoped memory containers** using a compact Binary Context
Format (BCF). Each scope is an independently encrypted container:

| Scope | Contents |
|-------|----------|
| **Global** | System prompts, core personality (`soul.md`) |
| **User** | Per-user facts, preferences, conversation history |
| **Group** | Shared context for teams or channels |
| **Session** | Ephemeral context for the current conversation |

Context loading follows the principle of least privilege: a request only loads the scopes it
needs. User context never leaks into another user's session. Group context is only available to
authorised members.

### The Soul and Identity

Sõber's personality, values, and communication style are defined in a layered `soul.md` system:

- **Base layer** — compiled into the binary; defines the agent's foundation
- **User layer** (`~/.sober/soul.md`) — full user-level override
- **Workspace layer** (`./.sober/soul.md`) — additive project-specific context; cannot
  contradict ethical boundaries

The agent evolves its per-user soul layer autonomously over time, adapting to your communication
style and preferences. Changes to the base soul require either high-confidence pattern detection
across many interactions, or explicit admin approval.

---

## Core Principles

**Security First.** Every boundary in Sõber is authenticated and encrypted. The system operates
on a zero-trust model: no component trusts another without verification. Prompt injection
attempts are classified and blocked. Detected attacks trigger actor lockout and alerting.

**Context Isolation.** User, group, and system contexts are stored in separate scoped BCF
containers and never concatenated raw. A memory leak between user scopes is treated as a
security event, not a bug.

**Minimal Context Loading.** Sõber loads only the context required for each operation. Long-term
memory is offloaded to a vector store (Qdrant) and retrieved by relevance, keeping each prompt
lean and focused.

**Self-Evolution.** Sõber improves itself through audited plugin installation, soul layer
adaptation, and — with admin approval — proposals to modify its own core behaviour. All
autonomous changes are audit-logged.

**Source of Truth.** Executable code is always stored as versioned source. Compiled binaries and
generated WASM artefacts are ephemeral; the source is permanent and auditable.

---

## System Overview

Sõber runs as four independent processes:

| Process | Role | Default Address |
|---------|------|----------------|
| `sober-web` | Reverse proxy and embedded frontend | `:8080` |
| `sober-api` | HTTP/WebSocket gateway | `:3000` |
| `sober-scheduler` | Autonomous tick engine and time-driven jobs | Unix socket |
| `sober-agent` | gRPC server; invoked by API and scheduler | Unix socket |

The frontend is a SvelteKit PWA that communicates with `sober-api` over HTTP and WebSocket.
The API gateway authenticates requests and routes them to `sober-agent` via gRPC over a Unix
domain socket. The scheduler runs independently and triggers agent work on time-based schedules.

---

## What These Docs Cover

| Section | Contents |
|---------|----------|
| **Getting Started** | Prerequisites, installation, configuration, and first run |
| **User Guide** | Conversations, memory management, plugins, workspaces, and the CLI |
| **Architecture** | System design, crate map, memory format, security model, and event delivery |
| **Plugins** | Writing, building, testing, and publishing plugins |
| **Contributing** | Development setup, code style, testing, and the PR workflow |

If you are setting up Sõber for the first time, continue to [Prerequisites](getting-started/prerequisites.md).

If you want to understand how the system is designed before deploying it, see the
[Architecture](architecture/overview.md) section.
