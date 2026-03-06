# 009 — sober-mind: Agent Identity, Prompt Assembly & Self-Evolution

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
- **Trait evolution** — autonomous refinement of per-user/group layers, gated adoption into base soul
- **Access control masks** — what each caller (scheduler, user, replica) can see and do
- **Self-modification governance** — graduated trust for memory, plugins, soul, and code changes

---

## 1. Prompt Assembly Engine

No hardcoded tiers. One engine composes the prompt dynamically based on trigger
context:

```
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
  └── ~/.sõber/SOUL.md          (user-level overrides/extensions)
       └── ./.sõber/SOUL.md     (workspace/project-level)
```

Each layer extends the previous. Merge rules differ by layer:

| Layer | Location | Override rules |
|-------|----------|---------------|
| Base | `backend/soul/SOUL.md` | Foundation --- defines everything |
| User | `~/.sõber/SOUL.md` | Full override of base. User controls their instance. |
| Workspace | `./.sõber/SOUL.md` | Additive only. Can override style and domain emphasis. Cannot contradict ethical boundaries or security rules from base/user layers. |

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
  ├── sober-core    (types, config)
  ├── sober-memory  (load/store soul layers in BCF)
  ├── sober-crypto  (sign soul change audit entries)
  └── sober-auth    (access control context for masks)

sober-agent depends on sober-mind
  (calls prompt assembly before every LLM invocation)
```

---

## 6. Impact on Existing Architecture

### New crate
- `sober-mind` added to workspace

### New files
- `backend/soul/SOUL.md` --- base agent identity document
- Resolution chain reads from `~/.sõber/SOUL.md` and `./.sõber/SOUL.md` at runtime

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
