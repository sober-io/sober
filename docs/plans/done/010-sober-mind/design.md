# 010 — sober-mind: Agent Identity, Prompt Assembly & Self-Evolution

> Soul management, dynamic prompt composition, access-controlled reasoning tiers,
> and autonomous trait evolution.
> Date: 2026-03-06

---

## Overview

`sober-mind` is the agent's identity and cognitive context layer. It owns:

- **SOUL.md** — base personality and values (human-readable, version-controlled)
- **Soul layers** — per-user and per-group adaptations stored in BCF memory
- **SOUL.md resolution chain** — base, user-level, and workspace-level layering
- **Prompt assembly** — dynamic composition of system prompts from soul + context + access controls
- **Injection detection** — classifies user input for prompt injection attempts before prompt assembly
- **Trait evolution** — autonomous refinement of per-user/group layers, gated adoption into base soul
- **Access control masks** — what each caller (scheduler, user, replica) can see and do
- **Self-modification governance** — graduated trust for memory, plugins, soul, and code changes

---

## 1. Prompt Assembly Engine

No hardcoded tiers. One engine composes the prompt dynamically based on trigger
context:

```
┌─────────────────────────────────────┐
│        Injection Classifier         │
│  (runs on user input first)         │
│  Reject / flag / pass               │
└──────────────┬──────────────────────┘
               ▼
┌─────────────────────────────────────┐
│          Prompt Assembly            │
│                                     │
│  SOUL.md (resolved from chain)      │
│  + Soul layer (user/group scope)    │
│  + Task context (what triggered)    │
│  + Access mask (what's allowed)     │
│  + Relevant memory (from BCF)       │
│  = Final system prompt              │
└─────────────────────────────────────┘
```

### Access Masks by Trigger Source

| Trigger | Access level |
|---------|-------------|
| Scheduler / internal | Full --- self-reasoning, memory modification, code proposals, evolution |
| Human interaction | Restricted --- no internal state visibility, deep requests forwarded |
| Replica delegation | Scoped --- only what the delegation grants |
| Admin (soberctl) | Full read, restricted write |

### Request Forwarding

When a human interaction requires deeper reasoning beyond the human-facing
access mask:

- **Simple forwarding (synchronous)** --- internal tier processes the request
  inline and returns a filtered result to the human-facing context.
- **Complex reasoning / evolution (async)** --- task queued via scheduler for the
  internal tier. Result surfaces later (next conversation, notification, etc.).

The human-facing context decides which path based on the nature of the request.

### Injection Detection

The injection classifier runs on all user input **before** prompt assembly.
It is the first line of defense against prompt injection attacks.

- **Module:** `injection.rs` within `sober-mind`
- **Input:** Raw user message text
- **Output:** `InjectionVerdict` — one of `Pass`, `Flagged(reason)`, or
  `Rejected(reason)`
- **On `Rejected`:** The message is not passed to prompt assembly. The caller
  receives an error indicating the input was rejected.
- **On `Flagged`:** The message proceeds to prompt assembly, but a canary
  warning is injected into the context so the agent is aware of the risk.
- **On `Pass`:** Normal flow.

The classifier uses heuristic pattern matching (instruction override patterns,
role-play injection, context boundary manipulation). It does NOT use an LLM
call — it must be fast and deterministic. The detection logic lives in
`sober-mind` because it is tightly coupled to prompt assembly: the classifier
needs to understand what constitutes a boundary violation in the context of the
prompt format being assembled.

```rust
pub enum InjectionVerdict {
    Pass,
    Flagged { reason: String },
    Rejected { reason: String },
}

pub fn classify_input(input: &str) -> InjectionVerdict;
```

---

## 2. SOUL.md --- Base Identity

A human-readable document defining the agent's core personality. Version-controlled
in git at `backend/soul/SOUL.md`.

Contents:
- Core values and behavioral principles
- Communication style and tone
- Ethical boundaries and safety guardrails
- Base capabilities and limitations
- Self-evolution guidelines (what it may and may not change about itself)

Admins can directly edit SOUL.md. The agent can propose changes but they require
either high confidence (consistent pattern across many contexts) or admin approval.

### SOUL.md Resolution Chain

Similar to `.gitconfig` or `.npmrc`, SOUL.md is resolved from multiple layers:

```
backend/soul/SOUL.md           (base --- shipped with the system)
  └── ~/.sober/SOUL.md          (user-level overrides/extensions)
       └── ./.sober/SOUL.md     (workspace/project-level)
```

Each layer extends the previous. Merge rules differ by layer:

| Layer | Location | Override rules |
|-------|----------|---------------|
| Base | `backend/soul/SOUL.md` | Foundation --- defines everything |
| User | `~/.sober/SOUL.md` | Full override of base. User controls their instance. |
| Workspace | `./.sober/SOUL.md` | Additive only. Can override style and domain emphasis. Cannot contradict ethical boundaries or security rules from base/user layers. |

The workspace restriction prevents a project-level SOUL.md from disabling safety
guardrails while still allowing contextual adjustments like "in this project, be
more formal and focus on Rust."

---

## 3. Soul Layers --- Per-User/Group Adaptations

Stored as BCF chunks in `sober-memory`, scoped to user or group:

- Communication preferences (formal vs casual, verbosity, language)
- Domain knowledge emphasis
- Interaction patterns learned over time
- Trust level and delegation history

These evolve **autonomously** --- the agent naturally adapts based on interactions.
All changes are logged for auditability. On demand, the full soul state (base +
resolution chain + dynamic layers) can be reconstructed into a human-readable
format for admin audit.

### BCF Integration

New chunk type: `Soul` (added to the existing chunk types: Fact, Conversation,
Skill, Preference, Embedding, Code).

Soul chunks are scoped like all other memory:
- Global scope: base soul traits (mirrors SOUL.md for fast access)
- User scope: per-user adaptations
- Group scope: per-group adaptations
- Session scope: not used for soul data (soul persists across sessions)

---

## 4. Trait Evolution & Adoption

```
Observation ──► Candidate Trait ──► Analysis ──► Decision
                                                    │
                                    ┌───────────────┼───────────────┐
                                    ▼               ▼               ▼
                               Auto-adopt      Queue for        Discard
                           (per-user/group     admin review   (insufficient
                            layer, high       (base soul,      evidence)
                            confidence)       low confidence)
```

### Confidence Scoring

- **Consistency** --- trait observed across N interactions / M contexts
- **Non-contradiction** --- doesn't conflict with existing base soul values
- **Stability** --- pattern persists over time, not a transient spike

### Graduated Trust for Self-Modification

| Target | Autonomy |
|--------|----------|
| Memory / soul layers (per-user/group) | Free --- autonomous |
| Plugins / skills | Autonomous with sandbox testing + audit pipeline |
| Base SOUL.md | High confidence auto-adopt OR admin approval |
| Core crate code | Propose only --- generates diff + reasoning + tests, queued for admin |

### Audit Logging

All proposed changes are logged regardless of outcome:
- Source contexts (anonymized cross-user for base soul proposals)
- Confidence score and reasoning
- Diff against current state
- Decision taken (adopted, queued, discarded)
- Timestamp

---

## 5. Crate Dependencies

```
sober-mind depends on:
  ├── sober-core    (types, config, AccessMask)
  ├── sober-memory  (load/store soul layers in BCF)
  └── sober-crypto  (sign soul change audit entries)

sober-agent depends on sober-mind
  (calls prompt assembly before every LLM invocation)
```

`sober-mind` does **not** depend on `sober-auth`. Access control is represented
by `AccessMask`, a type defined in `sober-core`. The caller (typically
`sober-agent`) is responsible for constructing the `AccessMask` from the
authenticated session context and passing it into `sober-mind`'s prompt
assembly functions. This keeps `sober-mind` decoupled from the authentication
stack.

---

## 6. Impact on Existing Architecture

### New crate
- `sober-mind` added to workspace

### New files
- `backend/soul/SOUL.md` --- base agent identity document
- Resolution chain reads from `~/.sober/SOUL.md` and `./.sober/SOUL.md` at runtime

### Modified crates
- `sober-agent` --- uses `sober-mind` for prompt assembly instead of hardcoded
  system prompts
- `sober-memory` --- new `Soul` BCF chunk type
- `sober-scheduler` --- internal-tier tasks trigger with full access mask

### New concepts in ARCHITECTURE.md
- sober-mind in crate map
- Prompt assembly engine in system architecture diagram
- Soul layer documentation
- SOUL.md resolution chain
