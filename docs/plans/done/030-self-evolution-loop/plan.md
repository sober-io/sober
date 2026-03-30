# #030: Self-Evolution Loop — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The agent autonomously evolves by generating WASM tools, creating skills, improving its own instructions, and scheduling automations based on conversation patterns.

**Architecture:** Single `evolution_events` table with type discriminator + JSONB payload. Four `propose_*` agent tools feed into a shared approval engine with configurable autonomy. Execution runs in `sober-agent` (has all dependencies). Detection via renamed `self_evolution_check` system job on a configurable 2h interval.

**Tech Stack:** Rust (sober-core, sober-db, sober-mind, sober-api, sober-cli, sober-agent), PostgreSQL, Protocol Buffers, Svelte 5, TypeScript

**Design spec:** `docs/plans/active/030-self-evolution-loop/design.md`

---

### Task 0: Move plan to active

**Files:**
- Move: `docs/plans/pending/030-self-evolution-loop/` → `docs/plans/active/030-self-evolution-loop/`

- [ ] **Step 1: Move plan folder**

```bash
git mv docs/plans/pending/030-self-evolution-loop docs/plans/active/030-self-evolution-loop
```

- [ ] **Step 2: Commit**

```
chore(plans): move 030-self-evolution-loop to active
```

---

### Task 1: Core types — enums, IDs, domain, input, config

Add all new types to `sober-core`. No database dependency yet — this is pure Rust types.

**Files:**
- Modify: `backend/crates/sober-core/src/types/ids.rs`
- Modify: `backend/crates/sober-core/src/types/enums.rs`
- Modify: `backend/crates/sober-core/src/types/domain.rs`
- Modify: `backend/crates/sober-core/src/types/input.rs`
- Modify: `backend/crates/sober-core/src/types/mod.rs`
- Modify: `backend/crates/sober-core/src/config.rs`

- [ ] **Step 1: Add `EvolutionEventId` to `ids.rs`**

```rust
define_id! {
    /// Unique identifier for an evolution event.
    EvolutionEventId
}
```

- [ ] **Step 2: Add enums to `enums.rs`**

Add `EvolutionType`, `EvolutionStatus`, `AutonomyLevel` following the existing enum pattern
(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type behind postgres
feature, serde rename_all = "lowercase" or "snake_case"):

```rust
/// The kind of self-evolution event.
///
/// Maps to the `evolution_type` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "evolution_type", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum EvolutionType {
    /// WASM binary tool via sober-plugin-gen.
    Plugin,
    /// Prompt-based skill (PluginKind::Skill).
    Skill,
    /// Instruction overlay file modification.
    Instruction,
    /// Scheduled job creation.
    Automation,
}

/// Lifecycle status of an evolution event.
///
/// Maps to the `evolution_status` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "evolution_status", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum EvolutionStatus {
    /// Agent created the proposal, awaiting approval.
    Proposed,
    /// Approved (auto or manual), queued for execution.
    Approved,
    /// Execution engine is processing.
    Executing,
    /// Evolution is live and in use.
    Active,
    /// Execution failed. Admin can retry.
    Failed,
    /// Admin rejected the proposal.
    Rejected,
    /// Previously active evolution was rolled back.
    Reverted,
}

/// Autonomy level for a type of evolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyLevel {
    /// Proposals auto-approve (skip proposed state).
    Auto,
    /// Proposals require admin approval.
    ApprovalRequired,
    /// Evolution type is disabled entirely.
    Disabled,
}
```

Add serde roundtrip tests for `EvolutionType`, `EvolutionStatus`, and `AutonomyLevel`.

- [ ] **Step 3: Add `EvolutionEvent` domain type to `domain.rs`**

Add imports for `EvolutionType`, `EvolutionStatus`, `EvolutionEventId`. Then:

```rust
/// A self-evolution event (proposed, active, or historical).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionEvent {
    /// Unique identifier.
    pub id: EvolutionEventId,
    /// Type of evolution.
    pub evolution_type: EvolutionType,
    /// Whose patterns triggered this (attribution, not ownership).
    pub user_id: Option<UserId>,
    /// Human-readable title.
    pub title: String,
    /// Agent's reasoning and evidence.
    pub description: String,
    /// LLM-generated confidence score (0.0–1.0).
    pub confidence: f32,
    /// Number of conversations that triggered detection.
    pub source_count: i32,
    /// Current lifecycle status.
    pub status: EvolutionStatus,
    /// Type-specific data (plugin manifest, skill content, etc.).
    pub payload: serde_json::Value,
    /// Execution result (plugin ID, skill path, job ID, error).
    pub result: Option<serde_json::Value>,
    /// Ordered status transitions with timestamps.
    pub status_history: serde_json::Value,
    /// Who approved/rejected (NULL = auto).
    pub decided_by: Option<UserId>,
    /// When the evolution was reverted.
    pub reverted_at: Option<DateTime<Utc>>,
    /// When the event was created.
    pub created_at: DateTime<Utc>,
    /// When the event was last updated.
    pub updated_at: DateTime<Utc>,
}
```

- [ ] **Step 4: Add `CreateEvolutionEvent` input to `input.rs`**

```rust
/// Input for creating an evolution event.
#[derive(Debug, Clone)]
pub struct CreateEvolutionEvent {
    /// Type of evolution.
    pub evolution_type: EvolutionType,
    /// Human-readable title.
    pub title: String,
    /// Agent's reasoning and evidence.
    pub description: String,
    /// Confidence score.
    pub confidence: f32,
    /// Source conversation count.
    pub source_count: i32,
    /// Initial status (proposed or approved for auto-approve).
    pub status: EvolutionStatus,
    /// Type-specific payload.
    pub payload: serde_json::Value,
    /// Whose patterns triggered this.
    pub user_id: Option<UserId>,
}
```

- [ ] **Step 5: Add `EvolutionConfig` to `config.rs` (interval only)**

Add after the last config struct. Add `pub evolution: EvolutionConfig` field to `AppConfig`.
Only the check interval lives in `AppConfig` — autonomy levels are DB-backed (see Step 5b).

```rust
pub struct EvolutionConfig {
    pub interval: String,
}
```

Default: `interval = "2h"`. Env var: `SOBER_EVOLUTION_INTERVAL`.

- [ ] **Step 5b: Add `EvolutionConfigRow` domain type to `domain.rs`**

Autonomy levels are stored in the `evolution_config` single-row table (not `AppConfig`):

```rust
/// DB-backed autonomy configuration for self-evolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionConfigRow {
    pub plugin_autonomy: AutonomyLevel,
    pub skill_autonomy: AutonomyLevel,
    pub instruction_autonomy: AutonomyLevel,
    pub automation_autonomy: AutonomyLevel,
    pub updated_at: DateTime<Utc>,
}
```

- [ ] **Step 6: Update `mod.rs` re-exports**

Add all new types to the appropriate re-export lines in `types/mod.rs`.

- [ ] **Step 7: Verify**

Run: `cd backend && cargo build -q -p sober-core`

- [ ] **Step 8: Commit**

```
feat(core): add evolution types, enums, config, and EvolutionEvent domain model
```

---

### Task 2: Migration — `evolution_events` table

**Files:**
- Create: `backend/migrations/20260329000001_create_evolution_events.sql`

- [ ] **Step 1: Write migration**

```sql
CREATE TYPE evolution_type AS ENUM ('plugin', 'skill', 'instruction', 'automation');
CREATE TYPE evolution_status AS ENUM (
  'proposed', 'approved', 'executing', 'active', 'failed', 'rejected', 'reverted'
);

CREATE TABLE evolution_events (
  id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  evolution_type  evolution_type NOT NULL,
  user_id         UUID REFERENCES users(id) ON DELETE SET NULL,
  title           TEXT NOT NULL,
  description     TEXT NOT NULL,
  confidence      REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
  source_count    INT NOT NULL DEFAULT 1,
  status          evolution_status NOT NULL DEFAULT 'proposed',
  payload         JSONB NOT NULL,
  result          JSONB,
  status_history  JSONB NOT NULL DEFAULT '[]',
  decided_by      UUID REFERENCES users(id),
  reverted_at     TIMESTAMPTZ,
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_evolution_events_status ON evolution_events(status);
CREATE INDEX idx_evolution_events_type_status ON evolution_events(evolution_type, status);
CREATE UNIQUE INDEX idx_evolution_events_no_duplicates
  ON evolution_events(evolution_type, lower(regexp_replace(title, '[^a-z0-9]+', '-', 'g')))
  WHERE status IN ('proposed', 'approved', 'executing', 'active');

CREATE TABLE evolution_config (
  id              BOOL PRIMARY KEY DEFAULT TRUE CHECK (id),
  plugin_autonomy TEXT NOT NULL DEFAULT 'approval_required',
  skill_autonomy  TEXT NOT NULL DEFAULT 'auto',
  instruction_autonomy TEXT NOT NULL DEFAULT 'approval_required',
  automation_autonomy TEXT NOT NULL DEFAULT 'auto',
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO evolution_config DEFAULT VALUES;
```

- [ ] **Step 2: Run migration**

Run: `cd backend && cargo sqlx migrate run` (Docker must be running)

- [ ] **Step 3: Commit**

```
feat(db): add evolution_events table and evolution_config singleton
```

---

### Task 3: `EvolutionRepo` trait + `PgEvolutionRepo`

**Files:**
- Modify: `backend/crates/sober-core/src/types/repo.rs` — add `EvolutionRepo` trait
- Modify: `backend/crates/sober-core/src/types/mod.rs` — re-export
- Modify: `backend/crates/sober-db/src/rows.rs` — add `EvolutionEventRow`
- Create: `backend/crates/sober-db/src/repos/evolution.rs` — `PgEvolutionRepo`
- Modify: `backend/crates/sober-db/src/repos/mod.rs` — add module
- Modify: `backend/crates/sober-db/src/lib.rs` — re-export

- [ ] **Step 1: Add `EvolutionRepo` trait to `repo.rs`**

Add imports for `EvolutionEventId`, `EvolutionType`, `EvolutionStatus` to the enum import
line. Then add after `PluginRepo`:

```rust
/// Evolution event operations.
pub trait EvolutionRepo: Send + Sync {
    /// Creates a new evolution event.
    fn create(
        &self,
        input: CreateEvolutionEvent,
    ) -> impl Future<Output = Result<EvolutionEvent, AppError>> + Send;

    /// Finds an event by ID.
    fn get_by_id(
        &self,
        id: EvolutionEventId,
    ) -> impl Future<Output = Result<EvolutionEvent, AppError>> + Send;

    /// Lists events with optional type and status filters.
    fn list(
        &self,
        r#type: Option<EvolutionType>,
        status: Option<EvolutionStatus>,
    ) -> impl Future<Output = Result<Vec<EvolutionEvent>, AppError>> + Send;

    /// Lists all active events (for detection context).
    fn list_active(
        &self,
    ) -> impl Future<Output = Result<Vec<EvolutionEvent>, AppError>> + Send;

    /// Updates status, appends to status_history, sets decided_by and reverted_at as appropriate.
    fn update_status(
        &self,
        id: EvolutionEventId,
        status: EvolutionStatus,
        decided_by: Option<UserId>,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Updates the result JSONB field (execution output or error).
    fn update_result(
        &self,
        id: EvolutionEventId,
        result: serde_json::Value,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Counts events auto-approved today (decided_by IS NULL, status IN (approved, executing, active)).
    fn count_auto_approved_today(
        &self,
    ) -> impl Future<Output = Result<i64, AppError>> + Send;

    /// Counts events currently in executing status.
    fn count_executing(
        &self,
    ) -> impl Future<Output = Result<i64, AppError>> + Send;

    /// Lists events ordered by created_at DESC for timeline view.
    fn list_timeline(
        &self,
        limit: i64,
        r#type: Option<EvolutionType>,
        status: Option<EvolutionStatus>,
    ) -> impl Future<Output = Result<Vec<EvolutionEvent>, AppError>> + Send;

    /// Returns the singleton evolution autonomy config from DB.
    fn get_config(
        &self,
    ) -> impl Future<Output = Result<EvolutionConfigRow, AppError>> + Send;

    /// Updates autonomy levels in the singleton evolution config row.
    fn update_config(
        &self,
        config: EvolutionConfigRow,
    ) -> impl Future<Output = Result<EvolutionConfigRow, AppError>> + Send;
}
```

- [ ] **Step 2: Add `EvolutionEventRow` to `rows.rs`**

Follow existing pattern (private row type, `#[derive(sqlx::FromRow)]`, `From<Row>` impl):

```rust
#[derive(sqlx::FromRow)]
pub(crate) struct EvolutionEventRow {
    pub id: Uuid,
    pub evolution_type: EvolutionType,
    pub user_id: Option<Uuid>,
    pub title: String,
    pub description: String,
    pub confidence: f32,
    pub source_count: i32,
    pub status: EvolutionStatus,
    pub payload: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub status_history: serde_json::Value,
    pub decided_by: Option<Uuid>,
    pub reverted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

Implement `From<EvolutionEventRow> for EvolutionEvent` following the `From<AuditLogRow>`
pattern — map UUIDs to newtype IDs via `::from_uuid()`.

- [ ] **Step 3: Create `repos/evolution.rs`**

Implement `PgEvolutionRepo` following the `PgAuditLogRepo` pattern:
- `new(pool: PgPool) -> Self`
- `create()` — generate UUIDv7, insert with `RETURNING *`, initialize `status_history` with first entry
- `get_by_id()` — `SELECT * WHERE id = $1`, return `NotFound` if missing
- `list()` — dynamic query building for optional type/status filters, `ORDER BY created_at DESC`
- `list_active()` — `SELECT * WHERE status = 'active' ORDER BY created_at DESC`
- `update_status()` — update status + updated_at, append to `status_history` JSONB via
  `jsonb_concat(status_history, $new_entry::jsonb)`, set `decided_by` if provided,
  set `reverted_at = now()` when status is `reverted`. Return `NotFound` if 0 rows affected.
- `update_result()` — `UPDATE SET result = $1, updated_at = now() WHERE id = $2`
- `count_auto_approved_today()` — `SELECT COUNT(*) WHERE decided_by IS NULL AND status IN (...) AND created_at >= current_date`
- `count_executing()` — `SELECT COUNT(*) WHERE status = 'executing'`
- `list_timeline()` — same as `list()` but always ordered `created_at DESC` with limit
- `get_config()` — `SELECT * FROM evolution_config WHERE id = TRUE`
- `update_config()` — `UPDATE evolution_config SET ... WHERE id = TRUE RETURNING *`

- [ ] **Step 4: Register in `repos/mod.rs` and `lib.rs`**

Add `pub mod evolution;` and re-export `PgEvolutionRepo`.

- [ ] **Step 5: Re-export `EvolutionRepo` in `types/mod.rs`**

- [ ] **Step 6: Regenerate sqlx offline data**

Run: `cd backend && cargo sqlx prepare --workspace`

- [ ] **Step 7: Verify**

Run: `cd backend && cargo build -q -p sober-db`

- [ ] **Step 8: Commit**

```
feat(db): add PgEvolutionRepo with CRUD, status lifecycle, and timeline queries
```

---

### Task 4: Agent proto definitions — `ExecuteEvolution` and `RevertEvolution` RPCs

**Files:**
- Modify: `backend/proto/sober/agent/v1/agent.proto`
- Rebuild generated code (automatic via build.rs)

- [ ] **Step 1: Add RPC definitions and message types to agent.proto**

Add to the `AgentService`:

```protobuf
rpc ExecuteEvolution(ExecuteEvolutionRequest) returns (ExecuteEvolutionResponse);
rpc RevertEvolution(RevertEvolutionRequest) returns (RevertEvolutionResponse);
```

Add messages:

```protobuf
message ExecuteEvolutionRequest {
  string evolution_event_id = 1;
}

message ExecuteEvolutionResponse {
  bool success = 1;
  string error = 2;
}

message RevertEvolutionRequest {
  string evolution_event_id = 1;
}

message RevertEvolutionResponse {
  bool success = 1;
  string error = 2;
}
```

- [ ] **Step 2: Verify proto compiles**

Run: `cd backend && cargo build -q -p sober-agent`

- [ ] **Step 3: Commit**

```
feat(agent): add ExecuteEvolution and RevertEvolution gRPC RPCs
```

---

### Task 5: Instruction overlay loading in `sober-mind`

**Files:**
- Modify: `backend/crates/sober-mind/src/instructions.rs` (or wherever prompt assembly loads instruction files)
- Modify: `backend/crates/sober-mind/src/evolution.rs` — add guardrail blocklist

- [ ] **Step 1: Add instruction overlay resolution**

In the prompt assembly code that loads instruction files via `include_str!()`, add an overlay
check: before using the compiled-in content, check `~/.sober/instructions/{relative_path}`.
If the overlay file exists, use its content instead.

Follow the existing `soul.md` resolution chain pattern (base → user → workspace). The overlay
directory is `~/.sober/instructions/`.

- [ ] **Step 2: Add `Mind::reload_instructions()` method**

Add a method that clears the workspace/overlay instruction cache so newly written overlays
take effect without restarting the agent process:

```rust
/// Clears cached overlay instructions, forcing re-read from disk on next prompt assembly.
pub fn reload_instructions(&self) { ... }
```

This is called by the execution engine (Task 6) after writing instruction overlay files.

- [ ] **Step 3: Add guardrail blocklist to `evolution.rs`**

Add a function that checks BOTH `category: guardrail` frontmatter AND a hardcoded blocklist
of critical files (`safety.md`):

```rust
/// Hardcoded files that can never be modified by evolution, regardless of frontmatter.
const GUARDRAIL_BLOCKLIST: &[&str] = &["safety.md"];

/// Returns true if the instruction file is a guardrail (cannot be modified by evolution).
/// Checks both YAML frontmatter `category: guardrail` and the hardcoded blocklist.
pub fn is_guardrail_file(relative_path: &str, content: &str) -> bool {
    // 1. Check hardcoded blocklist
    // 2. Parse YAML frontmatter between --- delimiters
    // 3. Check if category == "guardrail"
}
```

- [ ] **Step 4: Tests**

Test overlay resolution: when overlay exists, it takes precedence. When it doesn't, base is used.
Test `reload_instructions()`: after clearing cache, new overlay content is picked up.
Test guardrail detection: files with `category: guardrail` return true, hardcoded blocklist
files return true regardless of frontmatter, other files return false.

Run: `cd backend && cargo test -p sober-mind -q`

- [ ] **Step 5: Commit**

```
feat(mind): add instruction overlay loading, reload method, and guardrail blocklist
```

---

### Task 6: Execution engine — `execute_evolution()` and `revert_evolution()`

Core execution logic that all propose tools and gRPC handlers call. This is the heart of the
system — handles all four evolution types.

**Files:**
- Create: `backend/crates/sober-agent/src/evolution/mod.rs`
- Create: `backend/crates/sober-agent/src/evolution/executor.rs`
- Create: `backend/crates/sober-agent/src/evolution/revert.rs`
- Modify: `backend/crates/sober-agent/src/lib.rs` — add `evolution` module

- [ ] **Step 1: Create `evolution/mod.rs`**

```rust
pub mod executor;
pub mod revert;

pub use executor::execute_evolution;
pub use revert::revert_evolution;
```

- [ ] **Step 2: Create `evolution/executor.rs`**

Implement `execute_evolution(event: &EvolutionEvent, repos: &AgentRepos) -> Result<Value, AgentError>`:

1. Atomic status guard: `UPDATE SET status = 'executing' WHERE id = $1 AND status = 'approved'`.
   If 0 rows affected, skip (already picked up by concurrent API/CLI trigger).
2. Match on `event.evolution_type`:
   - `Plugin` → extract payload, call `sober-plugin-gen` pipeline, run audit, register
     plugin as `PluginKind::Wasm`, return `{ "plugin_id": "..." }`
   - `Skill` → extract payload, write skill file with frontmatter, register as
     `PluginKind::Skill`, reload `SkillCatalog`, return `{ "plugin_id": "...", "skill_path": "..." }`
   - `Instruction` → extract payload, validate not guardrail (call `is_guardrail_file()`),
     read existing overlay or base content for `previous_content`, write overlay file,
     call `Mind::reload_instructions()` so new overlay takes effect without restart,
     return `{}`
   - `Automation` → extract payload, create job via `JobRepo::create()`,
     return `{ "job_id": "..." }`
3. On success: update result, set status to `active`, send inbox notification
   to all admin users ("Evolution: created **{title}** {type} (confidence: {conf})")
4. On failure: update result with error, set status to `failed`

Inbox notifications use the existing `ConversationRepo::get_inbox()` +
`MessageRepo::create()` pattern to post a system message to each admin's inbox.

- [ ] **Step 3: Create `evolution/revert.rs`**

Implement `revert_evolution(event: &EvolutionEvent, repos: &AgentRepos) -> Result<(), AgentError>`:

1. Match on `event.evolution_type`:
   - `Plugin` → extract `plugin_id` from result, call `PluginRepo::delete()`
   - `Skill` → extract `plugin_id` + `skill_path`, delete plugin, delete skill file,
     reload `SkillCatalog`
   - `Instruction` → extract `previous_content` from payload, write it back to overlay
     (or delete overlay if `previous_content` is null)
   - `Automation` → extract `job_id` from result, call `JobRepo::cancel()`
2. On failure: log error but don't change status (already set to `reverted` by caller)

- [ ] **Step 4: Wire gRPC handlers**

In the agent's gRPC service implementation, add handlers for `ExecuteEvolution` and
`RevertEvolution` that load the event from DB and call `execute_evolution()` /
`revert_evolution()`.

- [ ] **Step 5: Verify**

Run: `cd backend && cargo build -q -p sober-agent`

- [ ] **Step 6: Commit**

```
feat(agent): add evolution execution engine with per-type handlers and revert logic
```

---

### Task 7: `propose_tool` and `propose_skill` agent tools

**Files:**
- Create: `backend/crates/sober-agent/src/tools/propose_tool.rs`
- Create: `backend/crates/sober-agent/src/tools/propose_skill.rs`
- Modify: `backend/crates/sober-agent/src/tools/mod.rs` — add modules + re-exports

- [ ] **Step 1: Create `propose_tool.rs`**

Implement the `Tool` trait. Metadata uses the JSON schema from the design spec (name,
description, capabilities, pseudocode, confidence, evidence, source_count, user_id).

`execute()` logic:
1. Parse and validate input fields
2. Check deduplication: the DB unique index on `(evolution_type, lower(regexp_replace(title, ...)))` prevents duplicates — handle the conflict error gracefully
3. Check rate limits: `count_auto_approved_today()` < 3
4. Read autonomy level from DB via `EvolutionRepo::get_config()` → `plugin_autonomy` (`Auto` → `approved`, `ApprovalRequired` → `proposed`, `Disabled` → return error)
5. Create event via `EvolutionRepo::create()`
6. Append to `status_history`
7. Log to audit trail
8. Return ToolOutput with status message

`context_modifying: false`, `internal: false`.

- [ ] **Step 2: Create `propose_skill.rs`**

Same pattern as `propose_tool.rs` but:
- Schema: name, description, prompt_template, confidence, evidence, source_count, user_id
- Reads autonomy from DB via `EvolutionRepo::get_config()` → `skill_autonomy`
- Evolution type: `EvolutionType::Skill`

- [ ] **Step 3: Register both tools**

Add `pub mod propose_tool;` and `pub mod propose_skill;` to `tools/mod.rs`.
Add to re-exports and wire into the tool registry bootstrap.

- [ ] **Step 4: Verify**

Run: `cd backend && cargo build -q -p sober-agent`

- [ ] **Step 5: Commit**

```
feat(agent): add propose_tool and propose_skill evolution tools
```

---

### Task 8: `propose_instruction_change` and `propose_automation` agent tools

**Files:**
- Create: `backend/crates/sober-agent/src/tools/propose_instruction.rs`
- Create: `backend/crates/sober-agent/src/tools/propose_automation.rs`
- Modify: `backend/crates/sober-agent/src/tools/mod.rs`

- [ ] **Step 1: Create `propose_instruction.rs`**

Same base pattern as Task 7 tools but:
- Schema: file, new_content, rationale, confidence, evidence, source_count, user_id
- **Dual guardrail check**: call `is_guardrail_file(relative_path, content)` which checks
  BOTH the hardcoded blocklist (`safety.md`) AND `category: guardrail` frontmatter.
  If either matches, return `ToolOutput` with error message, do NOT create an event.
- Reads autonomy from DB via `EvolutionRepo::get_config()` → `instruction_autonomy`
- Evolution type: `EvolutionType::Instruction`

- [ ] **Step 2: Create `propose_automation.rs`**

Same base pattern but:
- Schema: job_name, schedule, prompt, target_user_id, conversation_id, confidence, evidence, source_count
- Deduplication: check existing jobs for same user + similar schedule
- Reads autonomy from DB via `EvolutionRepo::get_config()` → `automation_autonomy`
- Evolution type: `EvolutionType::Automation`

- [ ] **Step 3: Register both tools and wire into registry**

- [ ] **Step 4: Verify**

Run: `cd backend && cargo build -q -p sober-agent`

- [ ] **Step 5: Commit**

```
feat(agent): add propose_instruction_change and propose_automation evolution tools
```

---

### Task 9: Update `self_evolution_check` system job

**Files:**
- Modify: `backend/crates/sober-agent/src/system_jobs.rs`

- [ ] **Step 1: Rename job and update schedule**

Rename `trait_evolution_check` to `self_evolution_check`. Change the schedule from the
hardcoded cron (`0 0 3 * * * *`) to use `EvolutionConfig::interval` (default `"every: 2h"`).

- [ ] **Step 2: Scheduler fires a generic Prompt job (trigger only)**

The scheduler fires a generic Prompt job with a fixed system prompt identifier
(e.g., `"self_evolution_check"`). The scheduler does NOT query evolution data
or construct the detection prompt — it is purely a trigger. All data gathering
and prompt construction happens inside the agent's handler (Step 3).

- [ ] **Step 3: Agent handler — data gathering + prompt construction**

When the agent receives the `self_evolution_check` prompt job, the handler:

1. Query `EvolutionRepo::list()` with status `approved` — execute pending approvals
   (Phase 1, same as before)
2. Query recent conversations via `MessageRepo` — summarize into compact format
   (user ID, tool calls used, topics, message count) for the last interval
3. Query `EvolutionRepo::list_active()` — get active evolutions with usage metrics
4. Build a structured context string from the gathered data
5. Construct the prompt dynamically with pre-loaded data (no `recall` tool needed):
   - Inject conversation summaries
   - Inject active evolution context
   - Instruct the LLM to analyze patterns and call `propose_*` tools
   - Limit: max 5 proposals per cycle

The LLM receives the data as context, not via tools — this is faster and
cheaper than having the agent call `recall` repeatedly.

- [ ] **Step 4: Execute auto-approved events**

After the LLM detection phase completes, query for any events in `approved`
status that were just created (auto-approved in Phase 3). Execute them
immediately within the same cycle via `execute_evolution()`. This ensures
auto-approved evolutions don't wait for the next 2h cycle.

The only events that wait between cycles are `proposed` events awaiting
admin approval.

- [ ] **Step 3: Remove old `trait_evolution_check` prompt**

Delete the old static prompt text.

- [ ] **Step 4: Verify**

Run: `cd backend && cargo build -q -p sober-agent`

- [ ] **Step 5: Commit**

```
feat(agent): rename to self_evolution_check with dynamic prompt and configurable interval
```

---

### Task 10: API routes — 6 evolution endpoints

**Files:**
- Create: `backend/crates/sober-api/src/routes/evolution.rs`
- Modify: `backend/crates/sober-api/src/routes/mod.rs`

- [ ] **Step 1: Create `evolution.rs` route module**

Six handlers following the existing thin-handler pattern:

```rust
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/evolution", get(list_events))
        .route("/evolution/{id}", get(get_event))
        .route("/evolution/{id}", patch(update_event))
        .route("/evolution/config", get(get_config))
        .route("/evolution/config", patch(update_config))
        .route("/evolution/timeline", get(get_timeline))
}
```

**`list_events`** — `GET /evolution?type=plugin&status=active` — query params optional,
calls `EvolutionRepo::list()`. Requires admin auth (`RequireAdmin`).

**`get_event`** — `GET /evolution/{id}` — calls `EvolutionRepo::get_by_id()`.

**`update_event`** — `PATCH /evolution/{id}` — body `{ "status": "approved" | "rejected" | "reverted" }`.
Validates allowed transitions:
- `proposed` or `failed` → `approved` (approve/retry)
- `proposed` → `rejected`
- `active` → `reverted`

For `approved`: update DB status, then call agent gRPC `ExecuteEvolution` for immediate execution.
For `reverted`: update DB status + set `reverted_at`, then call agent gRPC `RevertEvolution`.
Logs to audit trail.

**`get_config`** — `GET /evolution/config` — calls `EvolutionRepo::get_config()` to read
from the `evolution_config` DB table. Returns autonomy levels + `interval` from `AppConfig`.

**`update_config`** — `PATCH /evolution/config` — body `{ "plugin_autonomy": "auto", ... }`.
Persists autonomy levels to DB via `EvolutionRepo::update_config()`.

**`get_timeline`** — `GET /evolution/timeline?type=plugin&status=active&limit=50` —
calls `EvolutionRepo::list_timeline()`.

- [ ] **Step 2: Register in `routes/mod.rs`**

Add `pub mod evolution;` and `.merge(evolution::routes())`.

- [ ] **Step 3: Verify**

Run: `cd backend && cargo build -q -p sober-api`

- [ ] **Step 4: Commit**

```
feat(api): add evolution management endpoints (list, detail, approve/reject/revert, config, timeline)
```

---

### Task 11: CLI commands — `sober evolution`, `sober plugin`, `sober skill`

**Files:**
- Modify: `backend/crates/sober-cli/src/cli.rs` — add command definitions
- Create: `backend/crates/sober-cli/src/commands/evolution.rs`
- Create: `backend/crates/sober-cli/src/commands/plugin.rs`
- Create: `backend/crates/sober-cli/src/commands/skill.rs`
- Modify: `backend/crates/sober-cli/src/commands/mod.rs`
- Modify: `backend/crates/sober-cli/src/sober.rs` — dispatch

- [ ] **Step 1: Add command definitions to `cli.rs`**

Add three new variants to the `Command` enum:

```rust
/// Manage self-evolution events.
#[command(subcommand)]
Evolution(EvolutionCommand),

/// Manage installed plugins.
#[command(subcommand)]
Plugin(PluginCommand),

/// Manage skills.
#[command(subcommand)]
Skill(SkillCommand),
```

Define `EvolutionCommand` (List, Approve, Reject, Revert, Config),
`PluginCommand` (List, Enable, Disable, Remove), and
`SkillCommand` (List, Reload) using clap derive following the existing
`UserCommand`/`SchedulerCommand` patterns.

See design spec CLI section for exact argument definitions and output formats.

- [ ] **Step 2: Create `commands/evolution.rs`**

Handler connects to DB for reads/writes AND to the agent via gRPC over UDS for
execution triggers (same pattern as `commands/scheduler.rs` which connects to the
scheduler via UDS).

- `list` — DB only (read via `PgEvolutionRepo`)
- `config` — reads autonomy levels from DB via `EvolutionRepo::get_config()`, interval from `AppConfig`
- `approve` — update DB status + call agent gRPC `ExecuteEvolution` for immediate execution
- `reject` — DB only (update status)
- `revert` — update DB status + call agent gRPC `RevertEvolution` for immediate cleanup

Agent socket path: use `AppConfig::agent.socket` (e.g., `/run/sober/agent.sock`).
Follow the UDS connection pattern from `commands/scheduler.rs`.

- [ ] **Step 3: Create `commands/plugin.rs`**

Uses `PgPluginRepo`. Implements list (tabular output with kind/name/scope/status/origin),
enable/disable (update status), remove (delete).

- [ ] **Step 4: Create `commands/skill.rs`**

List: reads skill directory and prints skills with name/description/source.
Reload: writes a signal file or prints instruction to restart agent.

- [ ] **Step 5: Register modules and dispatch in `sober.rs`**

Add `pub mod evolution;`, `pub mod plugin;`, `pub mod skill;` to `commands/mod.rs`.
Add `run_evolution`, `run_plugin`, `run_skill` functions following `run_user` pattern
(create pool, dispatch to handler).

- [ ] **Step 6: Verify**

Run: `cd backend && cargo build -q -p sober-cli`

- [ ] **Step 7: Commit**

```
feat(cli): add sober evolution/plugin/skill subcommands
```

---

### Task 12: Frontend — types, service, settings layout

**Files:**
- Modify: `frontend/src/lib/types/index.ts` — add evolution types
- Create: `frontend/src/lib/services/evolution.ts` — API client
- Create: `frontend/src/routes/(app)/settings/+layout.svelte` — tab navigation
- Create: `frontend/src/routes/(app)/settings/+page.ts` — redirect
- Modify: `frontend/src/routes/(app)/settings/plugins/+page.svelte` — strip outer wrapper
- Modify: `frontend/src/routes/(app)/+layout.svelte` — sidebar link update

- [ ] **Step 1: Add TypeScript types**

```typescript
export type EvolutionType = 'plugin' | 'skill' | 'instruction' | 'automation';
export type EvolutionStatus = 'proposed' | 'approved' | 'executing' | 'active' | 'failed' | 'rejected' | 'reverted';
export type AutonomyLevel = 'auto' | 'approval_required' | 'disabled';

export interface EvolutionEvent {
    id: string;
    evolution_type: EvolutionType;
    user_id: string | null;
    title: string;
    description: string;
    confidence: number;
    source_count: number;
    status: EvolutionStatus;
    payload: Record<string, unknown>;
    result: Record<string, unknown> | null;
    status_history: Array<{ status: string; at: string; by?: string | null }>;
    decided_by: string | null;
    reverted_at: string | null;
    created_at: string;
    updated_at: string;
}

export interface EvolutionConfig {
    interval: string;
    plugin_autonomy: AutonomyLevel;
    skill_autonomy: AutonomyLevel;
    instruction_autonomy: AutonomyLevel;
    automation_autonomy: AutonomyLevel;
}
```

- [ ] **Step 2: Create evolution service**

```typescript
import { api } from '$lib/utils/api';
import type { EvolutionEvent, EvolutionConfig } from '$lib/types';

export const evolutionService = {
    list: (type?: string, status?: string) => {
        const params = new URLSearchParams();
        if (type) params.set('type', type);
        if (status) params.set('status', status);
        const qs = params.toString();
        return api<EvolutionEvent[]>(`/evolution${qs ? `?${qs}` : ''}`);
    },
    get: (id: string) => api<EvolutionEvent>(`/evolution/${id}`),
    update: (id: string, status: string) =>
        api<EvolutionEvent>(`/evolution/${id}`, {
            method: 'PATCH',
            body: JSON.stringify({ status })
        }),
    getConfig: () => api<EvolutionConfig>('/evolution/config'),
    updateConfig: (config: Partial<EvolutionConfig>) =>
        api<EvolutionConfig>('/evolution/config', {
            method: 'PATCH',
            body: JSON.stringify(config)
        }),
    timeline: (limit = 50, type?: string, status?: string) => {
        const params = new URLSearchParams({ limit: String(limit) });
        if (type) params.set('type', type);
        if (status) params.set('status', status);
        return api<EvolutionEvent[]>(`/evolution/timeline?${params}`);
    }
};
```

- [ ] **Step 3: Create settings layout with tab navigation**

Create `settings/+layout.svelte` with `<a>` tab links for Evolution and Plugins.
Active tab derived from `$page.url.pathname`. Uses `resolve()` from `$app/paths`.
See design spec Frontend section for the layout wireframe.

- [ ] **Step 4: Create settings redirect**

Create `settings/+page.ts` — `redirect(302, resolve('/settings/evolution'))`.

- [ ] **Step 5: Strip plugins page outer wrapper**

Remove the outer `<div>` with `mx-auto`, padding, and `<h1>` from
`settings/plugins/+page.svelte` since the shared layout provides those.

- [ ] **Step 6: Update sidebar link**

In `(app)/+layout.svelte` (~line 417), change:
```svelte
<a href={resolve('/settings/plugins')} ...>Plugins</a>
```
to:
```svelte
<a href={resolve('/settings')} ...>Settings</a>
```

- [ ] **Step 7: Verify**

Run: `cd frontend && pnpm check && pnpm build --silent`

- [ ] **Step 8: Commit**

```
feat(frontend): add settings tab layout, evolution types, and API service
```

---

### Task 13: Frontend — evolution management page

**Files:**
- Create: `frontend/src/routes/(app)/settings/evolution/+page.svelte`

- [ ] **Step 1: Create evolution page**

Three sections stacked vertically (see design spec wireframes):

**Autonomy Configuration** — four dropdowns (one per evolution type), Save button.
Loads from `evolutionService.getConfig()`, saves via `evolutionService.updateConfig()`.

**Pending Proposals** — only shown when events with `status === 'proposed'` exist.
Cards with type icon, title, badge, confidence, description, evidence.
Approve button → `evolutionService.update(id, 'approved')`.
Reject button → `evolutionService.update(id, 'rejected')`.
Card animates out after action.

**Active Evolutions** — filter bar (All, Plugins, Skills, Instructions, Automations).
Cards with type icon, title, description, usage metrics from `result`, confidence, date.
Revert button with confirm dialog → `evolutionService.update(id, 'reverted')`.
Failed evolutions shown with error badge and Retry button (→ `approved`).

**Compact Timeline** — 5 most recent events. "View all" link to `/settings/evolution/timeline`.

All data loaded via `$effect` on mount: `evolutionService.list()` + `evolutionService.getConfig()`.

- [ ] **Step 2: Verify**

Run: `cd frontend && pnpm check && pnpm build --silent`

- [ ] **Step 3: Commit**

```
feat(frontend): add evolution management page with config, proposals, and active evolutions
```

---

### Task 14: Frontend — timeline page

**Files:**
- Create: `frontend/src/routes/(app)/settings/evolution/timeline/+page.svelte`

- [ ] **Step 1: Create timeline page**

Vertical timeline visualization (see design spec wireframe). Each evolution event is a node
with a status branch showing the full lifecycle from `status_history`.

**Filters** — three dropdowns: Type (All/Plugin/Skill/Instruction/Automation),
Status (All/Proposed/Active/Failed/Rejected/Reverted), Time range (24h/7d/30d/All).

**Node structure:**
- Header: timestamp, title, type icon + badge
- Summary: description, confidence, source count
- Status branch: vertical chain from `status_history` array. Each entry shows timestamp +
  status label. Active entries show usage metrics. Failed entries show error message.
- Contextual actions: Approve/Reject (proposed), Revert (active), Retry (failed)

**Pagination** — "Load more" button, cursor-based on `created_at`.

Data from `evolutionService.timeline()`.

- [ ] **Step 2: Verify**

Run: `cd frontend && pnpm check && pnpm build --silent`

- [ ] **Step 3: Commit**

```
feat(frontend): add evolution timeline page with status branches and filters
```

---

### Task 15: Observability — metrics.toml, tracing, logging, dashboards

Add observability following the existing pattern: `metrics.toml` declarations per crate,
`tracing` spans/logs in code, then regenerate Grafana dashboards via `dashboard-gen`.

**Files:**
- Create: `backend/crates/sober-agent/metrics.toml` (or modify if it exists)
- Modify: `backend/crates/sober-agent/src/evolution/executor.rs`
- Modify: `backend/crates/sober-agent/src/evolution/revert.rs`
- Modify: `backend/crates/sober-agent/src/tools/propose_tool.rs`
- Modify: `backend/crates/sober-agent/src/tools/propose_skill.rs`
- Modify: `backend/crates/sober-agent/src/tools/propose_instruction.rs`
- Modify: `backend/crates/sober-agent/src/tools/propose_automation.rs`
- Regenerate: `infra/grafana/dashboards/generated/`

- [ ] **Step 1: Create `metrics.toml` declarations**

Add evolution metrics to `backend/crates/sober-agent/metrics.toml` following the
`sober-plugin/metrics.toml` pattern:

```toml
[crate]
name = "sober-agent"
dashboard_title = "Agent — Self-Evolution"

[[metrics]]
name = "sober_evolution_events_total"
type = "counter"
help = "Evolution events by type and status"
labels = ["type", "status"]
group = "Evolution"

[[metrics]]
name = "sober_evolution_execution_duration_seconds"
type = "histogram"
help = "Evolution execution latency by type"
labels = ["type"]
group = "Evolution"

[[metrics]]
name = "sober_evolution_cycle_duration_seconds"
type = "histogram"
help = "Full self-evolution check cycle duration"
labels = []
group = "Evolution"

[[metrics]]
name = "sober_evolution_proposals_total"
type = "counter"
help = "Proposals created by type and autonomy outcome"
labels = ["type", "autonomy"]
group = "Evolution"

[[metrics]]
name = "sober_evolution_auto_approved_today"
type = "gauge"
help = "Number of auto-approved evolutions today (rate limit tracking)"
labels = []
group = "Evolution"

[[metrics]]
name = "sober_evolution_executing_count"
type = "gauge"
help = "Currently executing evolutions (concurrency limit tracking)"
labels = []
group = "Evolution"

[[metrics]]
name = "sober_evolution_reverts_total"
type = "counter"
help = "Evolution reverts by type"
labels = ["type"]
group = "Evolution"
```

- [ ] **Step 2: Tracing spans**

Add `#[instrument]` or `tracing::info_span!` to key functions:

- `execute_evolution()` — span with `evolution_type`, `event_id`, `title`
- `revert_evolution()` — span with `evolution_type`, `event_id`
- `self_evolution_check` handler — top-level span covering all 4 phases
- Each `propose_*` tool `execute()` — span with tool name and proposal title

- [ ] **Step 3: Prometheus metrics in code**

Instrument the code to emit the metrics declared in `metrics.toml`:

```rust
// Proposal created
metrics::counter!("sober_evolution_events_total", "type" => type_str, "status" => status_str).increment(1);

// Execution duration
metrics::histogram!("sober_evolution_execution_duration_seconds", "type" => type_str)
    .record(duration.as_secs_f64());

// Cycle duration
metrics::histogram!("sober_evolution_cycle_duration_seconds").record(cycle_duration.as_secs_f64());

// Rate limit gauges (updated each cycle)
metrics::gauge!("sober_evolution_auto_approved_today").set(count as f64);
metrics::gauge!("sober_evolution_executing_count").set(executing as f64);
```

- [ ] **Step 4: Structured logging**

Add structured log events at each phase transition:

```rust
// Phase 1: executing approved
tracing::info!(event_id = %id, evolution_type = %etype, title = %title, "executing approved evolution");

// Phase 2: data gathering
tracing::info!(conversation_count = count, active_evolutions = active, "evolution data gathered");

// Phase 3: detection complete
tracing::info!(proposals = proposed, auto_approved = auto, "evolution detection complete");

// Phase 4: execution results
tracing::info!(event_id = %id, evolution_type = %etype, "evolution executed successfully");
tracing::warn!(event_id = %id, evolution_type = %etype, error = %err, "evolution execution failed");

// Revert
tracing::info!(event_id = %id, evolution_type = %etype, "evolution reverted");
```

- [ ] **Step 5: Regenerate Grafana dashboards from metrics.toml**

Run the dashboard generator to produce updated dashboard JSON from `metrics.toml` files:

```bash
cd tools/dashboard-gen && cargo run -q -- \
  --input ../../backend/crates \
  --dashboards-output ../../infra/grafana/dashboards/generated \
  --alerts-output ../../infra/prometheus/alerts/generated
```

Verify the generated `sober-agent.json` dashboard includes an "Evolution" panel group
with panels for: events total, execution duration, cycle duration, proposals, rate limits.

- [ ] **Step 6: Add "Evolution" row to curated overview dashboard**

The curated overview dashboard (`infra/grafana/dashboards/curated/overview.json`) has
hand-crafted panels for system-wide visibility. It currently has 10 row sections:
System Health, API Performance, Agent & LLM, Agent Details, Scheduler,
Connection Pools & Memory, Security, Skills & Knowledge, Tool Usage & Agent Efficiency,
LLM Token Economics.

Add a new **"Evolution"** row section (after "Skills & Knowledge") with these panels:

1. **Evolution Events** (time series) — `rate(sober_evolution_events_total[5m])` broken
   down by `type` label. Shows proposal/execution/failure rate over time.
2. **Active Evolutions** (stat) — `count(sober_evolution_events_total{status="active"})`
   by type. Quick count of active plugins, skills, instructions, automations.
3. **Execution Duration** (histogram heatmap or percentiles) —
   `sober_evolution_execution_duration_seconds` by type. Shows how long WASM compilation
   vs skill creation vs instruction writes take.
4. **Cycle Duration** (time series) — `sober_evolution_cycle_duration_seconds` p50/p95.
   Tracks full detection+execution cycle health.
5. **Rate Limits** (gauge) — `sober_evolution_auto_approved_today` and
   `sober_evolution_executing_count`. Shows current utilization against limits (3/day, 2 concurrent).
6. **Reverts** (stat) — `sober_evolution_reverts_total` by type. Tracks how often
   evolutions get rolled back.

Follow the existing panel JSON structure in `overview.json` (datasource `${datasource}`,
gridPos widths of 6/8/12, consistent color scheme).

- [ ] **Step 7: Verify**

Run: `cd backend && cargo build -q -p sober-agent`

- [ ] **Step 8: Commit**

```
feat(agent): add evolution observability — metrics.toml, tracing, dashboards
```

---

### Task 16: Documentation — ARCHITECTURE.md, mdBook, CLI docs

Update all documentation to reflect the evolution system.

**Files:**
- Modify: `ARCHITECTURE.md`
- Create: `docs/book/src/user-guide/evolution.md`
- Modify: `docs/book/src/user-guide/cli.md`
- Modify: `docs/book/src/architecture/agent-mind.md`
- Modify: `docs/book/src/SUMMARY.md`

- [ ] **Step 1: Update ARCHITECTURE.md**

Add "Self-Evolution" subsection under "Agent Mind" documenting:
- The 4-phase cycle (execute approved → gather data → detect patterns → execute auto-approved)
- Four evolution types and their execution handlers
- Instruction overlay mechanism
- Autonomy configuration (DB-backed)
- Execution triggers (auto/API/CLI)

Update the crate map entries for `sober-agent` (evolution tools, ExecuteEvolution RPC)
and `sober-mind` (instruction overlay loading, guardrail blocklist).

- [ ] **Step 2: Create `docs/book/src/user-guide/evolution.md`**

User-facing documentation covering:
- What self-evolution is and how it works (high-level)
- The four evolution types with examples
- Autonomy configuration (how to set per-type autonomy)
- Managing evolutions via the Settings UI (approve, reject, revert)
- Managing evolutions via CLI (`sober evolution list/approve/reject/revert/config`)
- Safety guardrails (what can't be modified)
- Rate limits

- [ ] **Step 3: Update `docs/book/src/user-guide/cli.md`**

Add sections for the three new CLI subcommands:
- `sober evolution` — list, approve, reject, revert, config
- `sober plugin` — list, enable, disable, remove
- `sober skill` — list, reload

Include command syntax and example output matching the design spec.

- [ ] **Step 4: Update `docs/book/src/architecture/agent-mind.md`**

Add evolution pipeline details: instruction overlay resolution chain,
guardrail protection, and how the detection cycle feeds into prompt assembly.

- [ ] **Step 5: Update `docs/book/src/SUMMARY.md`**

Add evolution entry to the User Guide section:

```markdown
- [Self-Evolution](user-guide/evolution.md)
```

Add after "Scheduling" in the User Guide section.

- [ ] **Step 6: Commit**

```
docs(030): add evolution docs to ARCHITECTURE.md, mdBook user guide, and CLI reference
```

---

### Task 17: Version bumps, final verification, PR

**Files:**
- Modify: Multiple `Cargo.toml` files
- Move: `docs/plans/active/030-self-evolution-loop/` → `docs/plans/done/`

- [ ] **Step 1: Bump crate versions**

Minor version bumps (`feat/` branch) for all affected crates:
- `sober-core`, `sober-db`, `sober-api`, `sober-cli`, `sober-agent`, `sober-mind`

- [ ] **Step 2: Regenerate sqlx offline data**

Run: `cd backend && cargo sqlx prepare --workspace`

- [ ] **Step 3: Full verification**

```bash
cd backend && cargo fmt --check -q && cargo clippy -q -- -D warnings && cargo test --workspace -q
cd frontend && pnpm check && pnpm test --silent
```

- [ ] **Step 4: Docker rebuild**

```bash
docker compose up -d --build --quiet-pull 2>&1 | tail -15
```

- [ ] **Step 5: Move plan to done**

```bash
git mv docs/plans/active/030-self-evolution-loop docs/plans/done/030-self-evolution-loop
```

- [ ] **Step 6: Commit**

```
chore(030): version bumps, move plan to done
```

- [ ] **Step 7: Create PR**

Branch: `feat/030-self-evolution`
Title: `#030: Self-evolution loop — tools, skills, instructions, automations`

Body should summarize:
- New `evolution_events` + `evolution_config` tables
- Four `propose_*` agent tools (tool, skill, instruction, automation)
- Execution engine with per-type handlers and revert logic
- `ExecuteEvolution` / `RevertEvolution` gRPC RPCs
- DB-backed configurable autonomy per evolution type
- 6 REST API endpoints for evolution management
- `sober evolution/plugin/skill` CLI commands
- Instruction overlay mechanism for runtime instruction modification
- Settings UI with tab navigation, evolution page, and visual timeline
- Deduplication (functional index + tool validation + prompt context)
- Rate limiting (5/cycle, 3 auto/day, 2 concurrent executing)
- Full audit trail and admin inbox notifications
- Observability: tracing spans, Prometheus metrics, structured logging
- Documentation: ARCHITECTURE.md, mdBook user guide + CLI reference
