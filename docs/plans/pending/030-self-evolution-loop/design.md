# #030: Self-Evolution Loop

## Overview

Complete the self-improvement loop: pattern detection from conversations → trait
evolution with confidence scoring → plugin generation and audit → safety
guardrails. Builds on existing stubs in `sober-mind` (evolution.rs, layers.rs)
and the scheduled `trait_evolution_check` system job.

**Depends on:** #019 sober-plugin (plugin crate must exist before plugin
generation can land). Trait evolution (Sections 1-3) can proceed independently.

**Deferred:** Core code proposals (agent proposes diffs to its own crate code).
Separate future plan.

---

## Section 1: Pattern Detection & Trait Evolution

### Trigger

The existing `trait_evolution_check` cron job (daily at 3 AM) dispatches a
Prompt job to the agent via gRPC.

### Flow

1. Scheduler fires `trait_evolution_check` → agent receives prompt as a
   Scheduler-triggered task (full access, internal sections visible).
2. Agent recalls recent conversations across users via the `recall` tool with
   scope filters. Identifies recurring patterns: tone preferences, domain focus,
   response length, formality, etc.
3. For each detected pattern the agent calls the new **`propose_trait`** tool
   with a structured candidate: `(user_id?, trait_key, trait_value, confidence,
   evidence)`.
4. `sober-mind/src/evolution.rs` receives the candidate:
   - Validates `trait_key` against the soul-layer allowlist.
   - Confidence ≥ 0.85 → `AutoAdopt` (store soul layer immediately).
   - Confidence < 0.85 → `QueueForReview`.
   - Creates an `EvolutionAuditEntry` in the audit log.
5. Rate limits enforced: max 3 proposals per user per cycle, max 2 auto-adopts
   per user per week (excess queued even if high confidence).

### Approval Flow

| Scope | Channel | Action |
|-------|---------|--------|
| User-scoped (user_id set) | Inbox notification + Settings panel | User accepts/dismisses in "Suggested adaptations" section |
| System-scoped (user_id NULL) | `sober evolution list/approve/reject` | Admin reviews via CLI |

User-scoped proposals that remain pending for 30 days expire automatically
(status → `expired`, logged in audit trail).

### Inbox Notification

When a user-scoped proposal is created, the agent posts a message to the user's
inbox conversation:

> "Based on our recent conversations, I'd like to adjust **response_length →
> concise**. You can review this in your settings."

The actual Accept/Dismiss action lives in the Settings panel — the inbox message
is a notification only.

---

## Section 2: Plugin System

### What a Plugin Is

A sandboxed program (script or binary) with declared capabilities, registered in
the agent's tool registry. Executed via `sober-sandbox` (bwrap).

### Plugin Structure (on disk)

```
~/.sober/plugins/<name>/
  plugin.toml        # metadata, capabilities, entry point
  main.py / main.sh  # the actual code
  tests/             # test cases (run during audit)
```

### Manifest (`plugin.toml`)

```toml
name = "summarize-pdf"
version = "0.1.0"
description = "Extract and summarize PDF content"
entry = "main.py"
origin = "agent"  # or "user", "system"

[capabilities]
network = false
filesystem = ["read"]
max_runtime_secs = 30

[input]
file_path = { type = "string", description = "Path to PDF" }

[output]
type = "string"
```

### Lifecycle

**Discover → Audit → Install → Monitor → Remove**

1. **Discover**: Agent calls `propose_plugin` tool, or user runs
   `sober plugin install <path>`.
2. **Audit** (in `sober-plugin` crate):
   - Validate manifest (required fields, sane capabilities).
   - Static scan: dangerous patterns (exec, eval, network calls) vs declared
     capabilities.
   - Sandbox test run with sample inputs via `sober-sandbox` (bwrap).
   - Behavioral check: stayed within declared capabilities? Exited cleanly?
3. **Install**: Registered in DB (`plugins` table), wrapped as a tool via
   `PluginToolAdapter` (analogous to `McpToolAdapter`), available to agent.
4. **Monitor**: Runtime enforcement via bwrap policy matching declared
   capabilities. Each invocation logged in audit trail.
5. **Remove**: `sober plugin remove <name>`. Tool unregistered, files kept
   for audit.

### Agent-Generated Plugins

- Agent detects repeated pattern ("I keep fetching URLs and extracting data").
- Calls `propose_plugin` with: name, description, capabilities, pseudocode.
- A Prompt job runs: LLM writes code, tests, and manifest.
- Audit pipeline runs automatically.
- If capabilities are a subset of agent's existing capabilities → auto-install.
- If new capabilities requested → queued for admin via `sober`.
- Rate limit: max 1 plugin proposal per day.

### Trust Levels

| Origin | Audit | Approval |
|--------|-------|----------|
| System | Pre-audited, shipped with release | None |
| Agent-generated | Full pipeline | Auto if capability subset, else admin |
| User-provided | Full pipeline | Always admin |

---

## Section 3: Audit Trail & Safety Guardrails

### Audit Logging

All evolution and plugin actions logged via existing `audit_log` table:

- `trait.proposed`, `trait.auto_adopted`, `trait.user_approved`,
  `trait.user_dismissed`, `trait.admin_approved`, `trait.admin_rejected`,
  `trait.expired`, `trait.reverted`
- `plugin.proposed`, `plugin.audited`, `plugin.installed`, `plugin.removed`,
  `plugin.execution`

### Rollback

- Soul layer changes are versioned (`previous_value` column).
- User can dismiss an adopted trait from settings (reverts it).
- `sober evolution revert <id>` for admin rollback (post-v1).

### Rate Limits

- Max 3 trait proposals per user per evolution check cycle.
- Max 2 auto-adoptions per user per week.
- Max 1 plugin proposal per day.

### Kill Switches (post-v1)

- `sober evolution pause/resume` — disable autonomous evolution.
- `sober plugin disable-generation` — prevent agent plugin proposals.
- Per-user: "Allow agent adaptations" toggle in settings.

---

## Section 4: Data Model

### New Enum Types

```sql
CREATE TYPE evolution_status AS ENUM (
  'pending', 'adopted', 'dismissed', 'expired', 'reverted'
);

CREATE TYPE plugin_status AS ENUM (
  'auditing', 'active', 'disabled', 'removed'
);
```

### evolution_proposals

```sql
CREATE TABLE evolution_proposals (
  id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id        UUID REFERENCES users(id) ON DELETE CASCADE,
  trait_key      TEXT NOT NULL CHECK (trait_key IN (
                   'tone', 'verbosity', 'domain_focus', 'formality_level',
                   'response_length', 'language', 'explanation_depth',
                   'code_style', 'humor'
                 )),
  trait_value    TEXT NOT NULL,
  confidence     REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
  evidence       TEXT NOT NULL,
  source_count   INT NOT NULL DEFAULT 1,
  status         evolution_status NOT NULL DEFAULT 'pending',
  previous_value TEXT,
  decided_by     UUID REFERENCES users(id),
  metadata       JSONB NOT NULL DEFAULT '{}',
  expires_at     TIMESTAMPTZ NOT NULL,
  created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_evolution_proposals_user ON evolution_proposals(user_id, status);
CREATE INDEX idx_evolution_proposals_pending ON evolution_proposals(status, expires_at)
  WHERE status = 'pending';
```

User-scoped when `user_id` is set, system-scoped when NULL. Application sets
`expires_at` (default 30 days from creation).

### plugins

```sql
CREATE TABLE plugins (
  id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name           TEXT NOT NULL,
  version        TEXT NOT NULL DEFAULT '0.1.0',
  description    TEXT NOT NULL,
  origin         TEXT NOT NULL CHECK (origin IN ('system', 'agent', 'user')),
  entry_point    TEXT NOT NULL,
  capabilities   JSONB NOT NULL DEFAULT '{}',
  input_schema   JSONB NOT NULL DEFAULT '{}',
  output_type    TEXT NOT NULL DEFAULT 'string',
  install_path   TEXT NOT NULL,
  status         plugin_status NOT NULL DEFAULT 'auditing',
  audit_result   JSONB,
  installed_by   UUID REFERENCES users(id),
  created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_plugins_name ON plugins(name) WHERE status != 'removed';
CREATE INDEX idx_plugins_status ON plugins(status);
```

---

## Section 5: API & CLI Surface (v1)

### API Routes

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/v1/evolution-proposals?status=pending` | List proposals for authenticated user |
| PATCH | `/api/v1/evolution-proposals/{id}` | Accept/dismiss (body: `{ "status": "adopted" \| "dismissed" }`) |

### sober Commands

| Command | Purpose |
|---------|---------|
| `sober evolution list` | List pending system-scoped proposals |
| `sober evolution approve <id>` | Approve a system-scoped proposal |
| `sober evolution reject <id>` | Reject a system-scoped proposal |

Plugin management via `sober plugin install/list/remove` — depends on #019.

---

## Section 6: Crate Changes

| Crate | Changes |
|-------|---------|
| `sober-core` | Domain types: `EvolutionProposal`, `Plugin`, `PluginCapabilities`. Repo traits: `EvolutionRepo`, `PluginRepo`. |
| `sober-db` | `PgEvolutionRepo`, `PgPluginRepo`. Migrations for new tables. |
| `sober-mind` | Replace `evaluate_candidate()` stub with confidence threshold logic. Add `propose_trait()` entry point. |
| `sober-plugin` | **New crate** (depends on #019): manifest parsing, audit pipeline, `PluginToolAdapter`, generation orchestration. |
| `sober-agent` | New tools: `propose_trait`, `propose_plugin`. Updated `trait_evolution_check` prompt. |
| `sober-cli` | `sober evolution list/approve/reject` subcommands. |
| `sober-api` | `evolution_proposals` route module (2 endpoints). |
| Frontend | Settings panel: "Suggested adaptations" section with Accept/Dismiss. |

---

## Implementation Phases

**Phase 1 — Trait Evolution (independent of #019):**
- Migrations, domain types, repo traits/impls
- `propose_trait` agent tool
- `evaluate_candidate()` real logic
- API routes + frontend settings section
- Inbox notification flow
- `sober evolution` commands
- Updated `trait_evolution_check` prompt

**Phase 2 — Plugin System (after #019 lands):**
- `sober-plugin` crate: manifest, audit, PluginToolAdapter
- `propose_plugin` agent tool
- Plugin generation job flow
- `sober plugin` commands
