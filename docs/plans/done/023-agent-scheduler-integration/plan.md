# Agent–Scheduler Integration Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire up agent–scheduler integration so jobs execute autonomously with typed payloads, workspace scoping, conversational management, result delivery to conversations, and system-level maintenance jobs.

**Architecture:** The scheduler stays a simple tick engine. The agent becomes the single orchestrator for all job execution — prompt-based (LLM), artifact-based (sandbox), and internal (direct crate calls). Jobs are scoped to workspaces and deliver results back to user conversations via the existing WebSocket/gRPC streaming pipeline.

**Tech Stack:** Rust, tonic/prost (gRPC), sqlx (PostgreSQL), bincode (payload serialization), tokio (async)

**Spec:** `docs/plans/pending/023-agent-scheduler-integration/design.md`

---

## File Structure

### New files

| File | Responsibility |
|------|---------------|
| `backend/crates/sober-core/src/types/job_payload.rs` | `JobPayload`, `ArtifactType`, `InternalOp` enums with bincode serde |
| `backend/crates/sober-agent/src/tools/scheduler.rs` | `SchedulerTools` — agent tool wrappers for job CRUD with authorization |
| `backend/crates/sober-agent/src/tools/mod.rs` | (modify) Re-export `scheduler` module |
| `backend/crates/sober-agent/src/system_jobs.rs` | System job definitions and idempotent registration |
| `backend/migrations/YYYYMMDDHHMMSS_agent_scheduler_integration.sql` | Schema migration |

### Modified files

| File | What changes |
|------|-------------|
| `backend/proto/sober/scheduler/v1/scheduler.proto` | Add fields (workspace_id, created_by, conversation_id, name_filter) + PauseJob/ResumeJob RPCs |
| `backend/proto/sober/agent/v1/agent.proto` | Add `artifact_ref` to Done message |
| `backend/crates/sober-core/src/types/domain.rs` | Extend `Job` with workspace_id, created_by, conversation_id |
| `backend/crates/sober-core/src/types/input.rs` | Extend `CreateJob` with workspace_id, created_by, conversation_id |
| `backend/crates/sober-core/src/types/repo.rs` | Extend `JobRepo::list_filtered()` params, `JobRunRepo::complete()` params |
| `backend/crates/sober-core/src/types/mod.rs` | Re-export `job_payload` module |
| `backend/crates/sober-core/Cargo.toml` | Add `bincode` dependency |
| `backend/crates/sober-db/src/repos/jobs.rs` | Update SQL for new columns in create, list_filtered, complete |
| `backend/crates/sober-db/src/repos/conversations.rs` | Add `find_latest_by_user_and_workspace()` |
| `backend/crates/sober-scheduler/src/grpc.rs` | Pass through new fields, implement PauseJob/ResumeJob RPCs |
| `backend/crates/sober-scheduler/src/engine.rs` | Forward workspace_id + conversation_id in ExecuteTaskRequest |
| `backend/crates/sober-agent/src/grpc.rs` | Rewrite `execute_task()` with payload dispatch |
| `backend/crates/sober-agent/src/agent.rs` | Add conversation resolution helper |
| `backend/crates/sober-agent/src/lib.rs` | Re-export system_jobs module |
| `backend/crates/sober-agent/src/main.rs` | Wire SchedulerTools, register system jobs on startup |
| `backend/crates/sober-agent/Cargo.toml` | Add `bincode` dependency |
| `backend/crates/sober-mind/src/assembly.rs` | Add `assemble_autonomous_prompt()` method |

---

## Chunk 1: Proto + Migration + Core Types

Foundation layer. No logic — just schema, proto definitions, and domain types that everything else builds on.

### Task 1: Proto changes — scheduler.proto

**Files:**
- Modify: `backend/proto/sober/scheduler/v1/scheduler.proto`

- [ ] **Step 1: Add new fields to Job message**

Add after `created_at` (field 10):
```protobuf
  string workspace_id = 11;
  string created_by = 12;
  string conversation_id = 13;
```

- [ ] **Step 2: Add new fields to CreateJobRequest**

Add after `payload` (field 5):
```protobuf
  string workspace_id = 6;
  string created_by = 7;
  string conversation_id = 8;
```

- [ ] **Step 3: Add filters to ListJobsRequest**

Add after existing fields:
```protobuf
  string workspace_id = 4;
  string name_filter = 5;
```

- [ ] **Step 4: Add PauseJob/ResumeJob messages and RPCs**

Add messages:
```protobuf
message PauseJobRequest { string job_id = 1; }
message PauseJobResponse { Job job = 1; }
message ResumeJobRequest { string job_id = 1; }
message ResumeJobResponse { Job job = 1; }
```

Add RPCs to `SchedulerService`:
```protobuf
  rpc PauseJob(PauseJobRequest) returns (PauseJobResponse);
  rpc ResumeJob(ResumeJobRequest) returns (ResumeJobResponse);
```

- [ ] **Step 5: Verify proto compiles**

Run: `cd backend && cargo build -q -p sober-scheduler`

### Task 2: Proto changes — agent.proto

**Files:**
- Modify: `backend/proto/sober/agent/v1/agent.proto`

- [ ] **Step 1: Add artifact_ref to Done message**

The existing `Done` message (line 66) has fields 1-3. Add:
```protobuf
  string artifact_ref = 4;
```

- [ ] **Step 2: Verify proto compiles**

Run: `cd backend && cargo build -q -p sober-agent`

### Task 3: Database migration

**Files:**
- Create: `backend/migrations/YYYYMMDDHHMMSS_agent_scheduler_integration.sql`

- [ ] **Step 1: Generate migration file**

Run: `cd backend && sqlx migrate add agent_scheduler_integration`

- [ ] **Step 2: Write migration SQL**

```sql
-- Migrate existing 'agent' owner_type rows before changing constraint
UPDATE jobs SET owner_type = 'system' WHERE owner_type = 'agent';

-- Add workspace, creator, and conversation tracking
ALTER TABLE jobs ADD COLUMN workspace_id UUID REFERENCES workspaces(id);
ALTER TABLE jobs ADD COLUMN created_by UUID REFERENCES users(id);
ALTER TABLE jobs ADD COLUMN conversation_id UUID REFERENCES conversations(id) ON DELETE SET NULL;

-- Expand owner_type to include 'group', drop unused 'agent'
ALTER TABLE jobs DROP CONSTRAINT jobs_owner_type_check;
ALTER TABLE jobs ADD CONSTRAINT jobs_owner_type_check
    CHECK (owner_type IN ('system', 'user', 'group'));

-- Add result artifact reference to job_runs
ALTER TABLE job_runs ADD COLUMN result_artifact_ref TEXT;

-- Index for workspace-scoped queries
CREATE INDEX idx_jobs_workspace ON jobs(workspace_id) WHERE workspace_id IS NOT NULL;
```

- [ ] **Step 3: Run migration against dev database**

Run: `cd backend && sqlx migrate run`
Expected: Migration succeeds. Verify with `psql` or `sqlx` that new columns exist.

- [ ] **Step 4: Regenerate sqlx offline data**

Run: `cd backend && cargo sqlx prepare --workspace -q`

- [ ] **Step 5: Commit**

```bash
git add backend/migrations/ backend/.sqlx/ backend/proto/
git commit -m "feat(scheduler): add proto fields and migration for agent-scheduler integration"
```

### Task 4: JobPayload types in sober-core

**Files:**
- Create: `backend/crates/sober-core/src/types/job_payload.rs`
- Modify: `backend/crates/sober-core/src/types/mod.rs`
- Modify: `backend/crates/sober-core/Cargo.toml`

- [ ] **Step 1: Add bincode dependency to sober-core**

Add to `[dependencies]` in `backend/crates/sober-core/Cargo.toml`:
```toml
bincode = "1"
```

- [ ] **Step 2: Create job_payload.rs**

```rust
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Discriminated job payload — determines execution path in agent.
/// Serialized with bincode into the `payload_bytes` column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobPayload {
    /// Natural language prompt executed via LLM with workspace context.
    Prompt {
        text: String,
        workspace_id: Option<Uuid>,
        model_hint: Option<String>,
    },
    /// Compiled artifact (WASM or script) executed in sandbox.
    Artifact {
        blob_ref: String,
        artifact_type: ArtifactType,
        workspace_id: Uuid,
        env: HashMap<String, String>,
    },
    /// Internal operation dispatched directly to a crate method.
    /// No LLM involved — deterministic execution.
    Internal { operation: InternalOp },
}

impl JobPayload {
    /// Serialize to bytes for storage in payload_bytes column.
    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize from bytes stored in payload_bytes column.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }
}

/// Type of artifact to execute in sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArtifactType {
    Wasm,
    Script,
}

/// Deterministic internal operations that don't need LLM mediation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InternalOp {
    MemoryPruning,
    SessionCleanup,
    VectorIndexOptimize,
    PluginAudit,
}
```

- [ ] **Step 3: Re-export from types/mod.rs**

Add `pub mod job_payload;` to `backend/crates/sober-core/src/types/mod.rs`.

- [ ] **Step 4: Verify it compiles**

Run: `cd backend && cargo build -q -p sober-core`

### Task 5: Extend Job domain type and CreateJob input

**Files:**
- Modify: `backend/crates/sober-core/src/types/domain.rs` (Job struct, lines 142-169)
- Modify: `backend/crates/sober-core/src/types/input.rs` (CreateJob struct, lines 53-70)

- [ ] **Step 1: Add fields to Job struct**

Add to the `Job` struct (after `owner_id` field):
```rust
    pub workspace_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub conversation_id: Option<Uuid>,
```

- [ ] **Step 2: Add fields to CreateJob input**

Add to the `CreateJob` struct:
```rust
    pub workspace_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub conversation_id: Option<Uuid>,
```

- [ ] **Step 3: Update existing `job_serializes_correctly` test**

The test at `domain.rs` line 442-459 constructs a `Job` without the new fields. Add the missing fields:
```rust
            workspace_id: None,
            created_by: None,
            conversation_id: None,
```

- [ ] **Step 4: Verify it compiles**

Run: `cd backend && cargo check -q`
Expected: Compile errors in sober-db and sober-scheduler where `Job` and `CreateJob` are constructed — that's expected and will be fixed in the next tasks.

### Task 6: Update repo traits

**Files:**
- Modify: `backend/crates/sober-core/src/types/repo.rs` (JobRepo trait, lines 148-196; JobRunRepo, lines 198-217)

- [ ] **Step 1: Update list_filtered signature**

The existing `list_filtered()` takes `owner_type: Option<&str>`, `owner_id: Option<Uuid>`, `status: Option<&str>`. Keep `status` as `Option<&str>` (matching existing callers in scheduler). Add `workspace_id` and `name_filter`:

```rust
fn list_filtered(
    &self,
    owner_type: Option<&str>,
    owner_id: Option<uuid::Uuid>,
    status: Option<&str>,
    workspace_id: Option<uuid::Uuid>,
    name_filter: Option<&str>,
) -> impl Future<Output = Result<Vec<Job>, AppError>> + Send;
```

- [ ] **Step 2: Update JobRunRepo::complete signature**

The existing signature is `complete(id: JobRunId, result: Vec<u8>, error: Option<String>)`. Add `result_artifact_ref` as a fourth parameter. Keep existing parameter types unchanged:

```rust
fn complete(
    &self,
    id: JobRunId,
    result: Vec<u8>,
    error: Option<String>,
    result_artifact_ref: Option<String>,
) -> impl Future<Output = Result<(), AppError>> + Send;
```

**Important:** Update all existing call sites in `sober-scheduler/src/engine.rs` (lines ~174 and ~279) to pass `None` as the new fourth argument.

- [ ] **Step 3: Verify trait compiles**

Run: `cd backend && cargo check -q -p sober-core`

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-core/
git commit -m "feat(core): add JobPayload types and extend Job/CreateJob with workspace and conversation fields"
```

### Task 7: Update sober-db repos

**Files:**
- Modify: `backend/crates/sober-db/src/repos/jobs.rs` (PgJobRepo, lines 28-49 create; lines 149-196 list_filtered; PgJobRunRepo complete)
- Modify: `backend/crates/sober-db/src/repos/conversations.rs`

- [ ] **Step 1: Update PgJobRepo::create() SQL**

Add workspace_id, created_by, conversation_id to the INSERT statement and the corresponding `$N` bind parameters.

- [ ] **Step 2: Update PgJobRepo::list_filtered()**

Add dynamic WHERE clauses for `workspace_id` and `name_filter`:
```rust
if let Some(ws_id) = workspace_id {
    query.push(" AND workspace_id = ");
    query.push_bind(ws_id);
}
if let Some(name) = name_filter {
    query.push(" AND name = ");
    query.push_bind(name);
}
```

- [ ] **Step 3: Update PgJobRepo row mapping**

All `sqlx::FromRow` or manual row reads for `Job` must include the new columns:
```rust
workspace_id: row.get("workspace_id"),
created_by: row.get("created_by"),
conversation_id: row.get("conversation_id"),
```

- [ ] **Step 4: Update PgJobRunRepo::complete()**

Add `result_artifact_ref` to the UPDATE SQL:
```sql
UPDATE job_runs
SET status = $2, result = $3, error = $4, finished_at = now(), result_artifact_ref = $5
WHERE id = $1
```

- [ ] **Step 5: Update ConversationRepo trait first**

Add the new method to the `ConversationRepo` trait in `sober-core/src/types/repo.rs`. Use the project's typed ID newtypes (`UserId`, `WorkspaceId`):
```rust
fn find_latest_by_user_and_workspace(
    &self,
    user_id: UserId,
    workspace_id: Option<WorkspaceId>,
) -> impl Future<Output = Result<Option<Conversation>, AppError>> + Send;
```

- [ ] **Step 6: Implement in conversation repo**

Add to `backend/crates/sober-db/src/repos/conversations.rs`. Use the typed IDs — extract inner `Uuid` with `.as_ref()` or `.into_inner()` for the SQL bind:
```rust
pub async fn find_latest_by_user_and_workspace(
    &self,
    user_id: UserId,
    workspace_id: Option<WorkspaceId>,
) -> Result<Option<Conversation>, AppError> {
    // Use fetch_optional — returns Ok(None) when no rows match
    // Match the existing query patterns in this file for column selection
    let row = if let Some(ws_id) = workspace_id {
        sqlx::query_as!(/* ... WHERE user_id = $1 AND workspace_id = $2 ORDER BY updated_at DESC LIMIT 1 */)
            .bind(user_id)
            .bind(ws_id)
            .fetch_optional(&*self.pool)
            .await?
    } else {
        sqlx::query_as!(/* ... WHERE user_id = $1 ORDER BY updated_at DESC LIMIT 1 */)
            .bind(user_id)
            .fetch_optional(&*self.pool)
            .await?
    };
    Ok(row)
}
```
Match the exact column list and type annotations from existing queries in this file (e.g., `list_by_user`).

- [ ] **Step 7: Verify compiles**

Run: `cd backend && cargo build -q -p sober-db`

- [ ] **Step 8: Regenerate sqlx offline data**

Run: `cd backend && cargo sqlx prepare --workspace -q`

- [ ] **Step 9: Commit**

```bash
git add backend/crates/sober-db/ backend/.sqlx/
git commit -m "feat(db): update job repos for workspace scoping and conversation delivery"
```

---

## Chunk 2: Scheduler Pass-Through

Update the scheduler to pass through the new fields and add PauseJob/ResumeJob RPCs.

### Task 8: Scheduler gRPC — new fields pass-through

**Files:**
- Modify: `backend/crates/sober-scheduler/src/grpc.rs` (create_job at lines 98-143, list_jobs at lines 157-180)

- [ ] **Step 1: Update create_job() to pass through new fields**

In the `create_job` RPC handler, extract `workspace_id`, `created_by`, `conversation_id` from the request and pass them to `CreateJob` input:
```rust
let workspace_id = parse_optional_uuid(&req.workspace_id)?;
let created_by = parse_optional_uuid(&req.created_by)?;
let conversation_id = parse_optional_uuid(&req.conversation_id)?;
```

Add a helper `parse_optional_uuid` if it doesn't exist:
```rust
fn parse_optional_uuid(s: &str) -> Result<Option<Uuid>, Status> {
    if s.is_empty() {
        Ok(None)
    } else {
        Uuid::parse_str(s).map(Some).map_err(|e| Status::invalid_argument(e.to_string()))
    }
}
```

- [ ] **Step 2: Update list_jobs() to use new filters**

Extract `workspace_id` and `name_filter` from request, pass to `list_filtered()`:
```rust
let workspace_id = parse_optional_uuid(&req.workspace_id)?;
let name_filter = if req.name_filter.is_empty() { None } else { Some(req.name_filter.as_str()) };
```

- [ ] **Step 3: Update Job proto conversion**

Where `Job` domain type is converted to the proto `Job` message, map the new fields:
```rust
workspace_id: job.workspace_id.map(|id| id.to_string()).unwrap_or_default(),
created_by: job.created_by.map(|id| id.to_string()).unwrap_or_default(),
conversation_id: job.conversation_id.map(|id| id.to_string()).unwrap_or_default(),
```

- [ ] **Step 4: Verify compiles**

Run: `cd backend && cargo build -q -p sober-scheduler`

### Task 9: Scheduler gRPC — PauseJob/ResumeJob RPCs

**Files:**
- Modify: `backend/crates/sober-scheduler/src/grpc.rs`

- [ ] **Step 1: Implement PauseJob RPC**

```rust
async fn pause_job(
    &self,
    request: Request<PauseJobRequest>,
) -> Result<Response<PauseJobResponse>, Status> {
    let req = request.into_inner();
    let job_id = Uuid::parse_str(&req.job_id)
        .map_err(|e| Status::invalid_argument(e.to_string()))?;

    self.job_repo
        .update_status(job_id, JobStatus::Paused)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let job = self.job_repo
        .get_by_id(job_id)
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .ok_or_else(|| Status::not_found("Job not found"))?;

    Ok(Response::new(PauseJobResponse {
        job: Some(job_to_proto(job)),  // job_to_proto takes by value, not reference
    }))
}
```

- [ ] **Step 2: Implement ResumeJob RPC**

Same pattern as PauseJob but sets status to `Active` and recalculates `next_run_at`:
```rust
async fn resume_job(
    &self,
    request: Request<ResumeJobRequest>,
) -> Result<Response<ResumeJobResponse>, Status> {
    let req = request.into_inner();
    let job_id = Uuid::parse_str(&req.job_id)
        .map_err(|e| Status::invalid_argument(e.to_string()))?;

    self.job_repo
        .update_status(job_id, JobStatus::Active)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    // Recalculate next_run_at from now
    let job = self.job_repo
        .get_by_id(job_id)
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .ok_or_else(|| Status::not_found("Job not found"))?;

    let schedule = JobSchedule::parse(&job.schedule)
        .map_err(|e| Status::internal(e.to_string()))?;
    let next_run = schedule
        .next_run_after(Utc::now())
        .ok_or_else(|| Status::internal("Could not calculate next run"))?;

    self.job_repo
        .update_next_run(job_id, next_run)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let updated_job = self.job_repo
        .get_by_id(job_id)
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .ok_or_else(|| Status::not_found("Job not found"))?;

    Ok(Response::new(ResumeJobResponse {
        job: Some(job_to_proto(updated_job)),  // by value
    }))
}
```

- [ ] **Step 3: Verify compiles**

Run: `cd backend && cargo build -q -p sober-scheduler`

### Task 10: Scheduler engine — forward new fields

**Files:**
- Modify: `backend/crates/sober-scheduler/src/engine.rs` (execute_via_agent at lines 215-223)

- [ ] **Step 1: Update ExecuteTaskRequest construction**

In `execute_via_agent()`, change the `ExecuteTaskRequest` to forward workspace_id and conversation_id from the Job. **Important:** These fields are `optional string` in the proto, so the Rust types are `Option<String>`, not `String`. Match the existing `user_id` pattern:
```rust
let request = ExecuteTaskRequest {
    task_id: job.id.to_string(),
    task_type: "scheduled_job".into(),
    payload: job.payload_bytes.clone(),
    caller_identity: "scheduler".into(),
    user_id: job.owner_id.map(|id| id.to_string()),
    conversation_id: job.conversation_id.map(|id| id.to_string()),
    workspace_id: job.workspace_id.map(|id| id.to_string()),
};
```

- [ ] **Step 2: Verify compiles**

Run: `cd backend && cargo build -q -p sober-scheduler`

- [ ] **Step 3: Run scheduler tests**

Run: `cd backend && cargo test -p sober-scheduler -q`

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-scheduler/
git commit -m "feat(scheduler): pass through workspace/conversation fields and add PauseJob/ResumeJob RPCs"
```

---

## Chunk 3: Agent Autonomous Execution

The core change — rewrite `execute_task()` to dispatch based on typed payload.

### Task 11: Add bincode to sober-agent

**Files:**
- Modify: `backend/crates/sober-agent/Cargo.toml`

- [ ] **Step 1: Add bincode dependency**

Add to `[dependencies]`:
```toml
bincode = "1"
```

- [ ] **Step 2: Verify compiles**

Run: `cd backend && cargo build -q -p sober-agent`

### Task 12: Add assemble_autonomous_prompt to sober-mind

**Files:**
- Modify: `backend/crates/sober-mind/src/assembly.rs` (Mind struct at lines 27-30, assemble at lines 46-102)

- [ ] **Step 1: Write test for autonomous prompt assembly**

Create test in `backend/crates/sober-mind/src/assembly.rs` `#[cfg(test)]` module. **Note:** `Message` requires all fields (`id`, `conversation_id`, `role`, `content`, `tool_calls`, `tool_result`, `token_count`, `created_at`). The method under test must construct full `Message` structs matching the pattern at assembly.rs lines 73-82:
```rust
#[tokio::test]
async fn test_assemble_autonomous_prompt_returns_system_and_task() {
    let soul_resolver = SoulResolver::new(
        PathBuf::from("../../soul/SOUL.md"),
        None,
        None,
    );
    let mind = Mind::new(soul_resolver);
    let caller = CallerContext {
        user_id: None,
        trigger: TriggerKind::Scheduler,
        permissions: vec![],
        scope_grants: vec![],
        workspace_id: None,
    };

    let result = mind.assemble_autonomous_prompt("Summarize recent activity", &caller).await;
    assert!(result.is_ok());
    let messages = result.unwrap();
    // Should have at least a system message and a user message (the task)
    assert!(messages.len() >= 2);
    // Last message should contain the task text
    let last = messages.last().unwrap();
    assert!(last.content.contains("Summarize recent activity"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd backend && cargo test -p sober-mind -q -- test_assemble_autonomous_prompt`
Expected: FAIL — method does not exist.

- [ ] **Step 3: Implement assemble_autonomous_prompt**

Add to `Mind` impl in `backend/crates/sober-mind/src/assembly.rs`. **Must be `async`** because `soul_resolver.resolve()` is async. Construct full `Message` structs matching the existing pattern (lines 73-82):
```rust
/// Assemble a prompt for autonomous (non-conversational) execution.
/// Loads SOUL.md chain and builds system prompt — no conversation history.
/// The task text becomes the sole user message.
pub async fn assemble_autonomous_prompt(
    &self,
    task: &str,
    caller: &CallerContext,
) -> Result<Vec<Message>, MindError> {
    // 1. Resolve SOUL.md layers (async)
    let soul = self.soul_resolver.resolve().await?;

    // 2. Apply access mask based on caller trigger
    let masked = apply_access_mask(&soul, caller);

    // 3. Build system prompt (no tools for autonomous execution)
    let system_prompt = build_system_prompt(&masked, &[]);

    // 4. Return system message + task as user message
    // Construct full Message structs matching existing pattern:
    Ok(vec![
        Message {
            id: MessageId::new(),
            conversation_id: sober_core::ConversationId::new(),
            role: MessageRole::System,
            content: system_prompt,
            tool_calls: None,
            tool_result: None,
            token_count: None,
            created_at: chrono::Utc::now(),
        },
        Message {
            id: MessageId::new(),
            conversation_id: sober_core::ConversationId::new(),
            role: MessageRole::User,
            content: task.to_string(),
            tool_calls: None,
            tool_result: None,
            token_count: None,
            created_at: chrono::Utc::now(),
        },
    ])
}
```

**Deviation from spec:** The spec signature takes `context: &WorkspaceContext` as a parameter. This is omitted here because `Mind` doesn't own workspace loading — the agent loads workspace context and passes the resolved SOUL path to `SoulResolver`. If workspace-specific SOUL resolution is needed, pass it through the existing `SoulResolver` constructor or add a workspace SOUL path parameter later.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd backend && cargo test -p sober-mind -q -- test_assemble_autonomous_prompt`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-mind/
git commit -m "feat(mind): add assemble_autonomous_prompt for non-conversational execution"
```

### Task 13: Conversation resolution helper in agent

**Files:**
- Modify: `backend/crates/sober-agent/src/agent.rs` (Agent struct at lines 95-117)

- [ ] **Step 1: Add resolve_delivery_conversation method**

Add to `Agent` impl. **Note:** `ConversationRepo::get_by_id` returns `Result<Conversation, AppError>` (not `Option`). A missing conversation returns `Err(AppError::NotFound(...))`, not `Ok(None)`. Handle accordingly:
```rust
/// Resolve which conversation to deliver job results to.
/// Tries the original conversation_id first, falls back to user's latest
/// conversation in the same workspace.
pub async fn resolve_delivery_conversation(
    &self,
    conversation_id: Option<ConversationId>,
    user_id: UserId,
    workspace_id: Option<WorkspaceId>,
) -> Option<ConversationId> {
    // Try the original conversation first
    if let Some(cid) = conversation_id {
        // get_by_id returns Err on not-found, Ok on found
        if self.conversation_repo.get_by_id(cid).await.is_ok() {
            return Some(cid);
        }
    }
    // Fallback: user's most recent conversation in the same workspace
    self.conversation_repo
        .find_latest_by_user_and_workspace(user_id, workspace_id)
        .await
        .ok()
        .flatten()
        .map(|c| c.id)
}
```

- [ ] **Step 2: Verify compiles**

Run: `cd backend && cargo check -q -p sober-agent`

### Task 14: Rewrite execute_task() with payload dispatch

**Files:**
- Modify: `backend/crates/sober-agent/src/grpc.rs` (execute_task at lines 123-211)

- [ ] **Step 1: Update AgentEvent::Done to include artifact_ref**

Find where `AgentEvent::Done` is defined (likely `backend/crates/sober-agent/src/stream.rs` or similar) and add `artifact_ref: Option<String>` to the variant. Update all existing construction sites to pass `artifact_ref: None`.

- [ ] **Step 2: Rewrite execute_task() method**

Replace the existing `execute_task()` body with payload-aware dispatch. The method:

1. Parses `user_id`, `workspace_id`, `conversation_id` from the request (existing code does this already).
2. Tries to deserialize `request.payload` as `JobPayload` via `JobPayload::from_bytes()`.
3. If deserialization succeeds, dispatches based on variant:
   - `Prompt`: resolve conversation → assemble autonomous prompt via `mind.assemble_autonomous_prompt()` → call LLM → store result artifact → stream events
   - `Artifact`: resolve blob path → build sandbox command → execute via `BwrapSandbox` → store result → stream events
   - `Internal`: dispatch to appropriate crate method → stream result
4. If deserialization fails, falls back to the existing behavior (treat payload as UTF-8 prompt string, delegate to `agent.handle_message()`).

Key implementation details:
- Use `mpsc::channel` + `tokio::spawn` pattern matching existing code
- Resolve delivery conversation via `agent.resolve_delivery_conversation()`
- For Prompt jobs, store assistant message in messages table linked to resolved conversation_id
- Prepend job context header: `[Scheduled job: "{name}" — {schedule}]\n\n`

- [ ] **Step 3: Verify compiles**

Run: `cd backend && cargo build -q -p sober-agent`

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-agent/
git commit -m "feat(agent): rewrite execute_task with typed payload dispatch and conversation delivery"
```

---

## Chunk 4: Agent Scheduler Tools (Conversational Job Management)

### Task 15: Create SchedulerTools

**Files:**
- Create: `backend/crates/sober-agent/src/tools/scheduler.rs`
- Modify: `backend/crates/sober-agent/src/tools/mod.rs`

- [ ] **Step 1: Create tools/scheduler.rs with SchedulerTools struct**

**Known deviation from spec:** The design specifies an `AuthorizationService` trait for group membership/admin checks. This is deferred — group authorization uses TODO placeholders. User-level authorization (creator check) is fully implemented. Group auth will be wired when the RBAC system is built.

**Proto import:** Use the re-export from the agent crate's grpc module: `use crate::grpc::scheduler_proto;`

```rust
use sober_core::types::{
    domain::{Job, JobStatus},
    job_payload::{ArtifactType, JobPayload},
};
use uuid::Uuid;

use crate::SharedSchedulerClient;
use crate::grpc::scheduler_proto;

/// Job action for authorization checks.
#[derive(Debug, Clone, Copy)]
pub enum JobAction {
    View,
    Create,
    Modify,
    Cancel,
}

/// Agent tools for managing scheduled jobs via the scheduler gRPC service.
pub struct SchedulerTools {
    scheduler_client: SharedSchedulerClient,
}

impl SchedulerTools {
    pub fn new(scheduler_client: SharedSchedulerClient) -> Self {
        Self { scheduler_client }
    }

    /// Check authorization for a job action.
    fn authorize(
        &self,
        caller_user_id: Uuid,
        job: &Job,
        action: JobAction,
    ) -> Result<(), String> {
        match (job.owner_type.as_str(), action) {
            ("user", _) => {
                if job.created_by != Some(caller_user_id) {
                    return Err("Forbidden: not the job owner".into());
                }
            }
            ("group", JobAction::View | JobAction::Create) => {
                // TODO: check group membership via auth service
                // For now, allow — will be wired when AuthorizationService is implemented
            }
            ("group", _) => {
                if job.created_by != Some(caller_user_id) {
                    // TODO: check group admin status
                    return Err("Forbidden: not the job creator or group admin".into());
                }
            }
            ("system", _) => return Err("System jobs cannot be managed via conversation".into()),
            _ => return Err("Unknown owner type".into()),
        }
        Ok(())
    }
}
```

- [ ] **Step 2: Implement create_job tool method**

```rust
impl SchedulerTools {
    pub async fn create_job(
        &self,
        name: &str,
        schedule: &str,
        payload: JobPayload,
        caller_user_id: Uuid,
        workspace_id: Option<Uuid>,
        conversation_id: Option<Uuid>,
    ) -> Result<String, String> {
        let payload_bytes = payload.to_bytes().map_err(|e| e.to_string())?;

        let owner_type = if workspace_id.is_some() {
            "user" // TODO: detect group workspace
        } else {
            "user"
        };

        let req = scheduler_proto::CreateJobRequest {
            name: name.into(),
            owner_type: owner_type.into(),
            owner_id: caller_user_id.to_string(),
            schedule: schedule.into(),
            payload: payload_bytes,
            workspace_id: workspace_id.map(|id| id.to_string()).unwrap_or_default(),
            created_by: caller_user_id.to_string(),
            conversation_id: conversation_id.map(|id| id.to_string()).unwrap_or_default(),
        };

        let mut client = self.scheduler_client.write().await;
        let client = client.as_mut().ok_or("Scheduler not connected")?;
        let response = client
            .create_job(req)
            .await
            .map_err(|e| e.to_string())?;
        let job = response.into_inner();

        Ok(format!(
            "Created job '{}' ({}). Next run: {}",
            job.name, job.id, job.next_run_at
        ))
    }
}
```

- [ ] **Step 3: Implement list, get, cancel, pause, resume, get_runs methods**

Each follows the same pattern: acquire scheduler client lock, call the appropriate RPC, format the response as a string. Use `authorize()` for operations that modify existing jobs.

- [ ] **Step 4: Export from tools/mod.rs**

Add `pub mod scheduler;` to `backend/crates/sober-agent/src/tools/mod.rs`.

- [ ] **Step 5: Verify compiles**

Run: `cd backend && cargo build -q -p sober-agent`

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-agent/src/tools/
git commit -m "feat(agent): add SchedulerTools for conversational job management"
```

---

## Chunk 5: System Jobs + Wiring

### Task 16: System job registration

**Files:**
- Create: `backend/crates/sober-agent/src/system_jobs.rs`
- Modify: `backend/crates/sober-agent/src/lib.rs`

- [ ] **Step 1: Create system_jobs.rs**

```rust
use sober_core::types::job_payload::{InternalOp, JobPayload};
use tracing::{info, warn};

use crate::SharedSchedulerClient;

struct SystemJobDef {
    name: &'static str,
    schedule: &'static str,
    payload: JobPayload,
}

const SYSTEM_JOBS: &[fn() -> SystemJobDef] = &[
    || SystemJobDef {
        name: "memory_pruning",
        schedule: "every: 1h",
        payload: JobPayload::Internal {
            operation: InternalOp::MemoryPruning,
        },
    },
    || SystemJobDef {
        name: "session_cleanup",
        schedule: "every: 6h",
        payload: JobPayload::Internal {
            operation: InternalOp::SessionCleanup,
        },
    },
    || SystemJobDef {
        name: "trait_evolution_check",
        schedule: "0 3 * * *",
        payload: JobPayload::Prompt {
            text: "Review interaction patterns across users. Propose SOUL.md \
                   trait adjustments if high-confidence patterns detected."
                .into(),
            workspace_id: None,
            model_hint: None,
        },
    },
    || SystemJobDef {
        name: "plugin_audit",
        schedule: "0 4 * * MON",
        payload: JobPayload::Internal {
            operation: InternalOp::PluginAudit,
        },
    },
    || SystemJobDef {
        name: "vector_index_optimize",
        schedule: "0 2 * * SUN",
        payload: JobPayload::Internal {
            operation: InternalOp::VectorIndexOptimize,
        },
    },
];

/// Register predefined system jobs idempotently.
/// Skips any job that already exists by name.
pub async fn register_system_jobs(scheduler_client: &SharedSchedulerClient) {
    let mut client_guard = scheduler_client.write().await;
    let Some(client) = client_guard.as_mut() else {
        warn!("Scheduler not connected — skipping system job registration");
        return;
    };

    for job_fn in SYSTEM_JOBS {
        let def = job_fn();

        // Check if already registered
        let existing = client
            .list_jobs(scheduler_proto::ListJobsRequest {
                owner_type: "system".into(),
                name_filter: def.name.into(),
                ..Default::default()
            })
            .await;

        match existing {
            Ok(resp) if !resp.into_inner().jobs.is_empty() => {
                info!(name = def.name, "System job already registered, skipping");
                continue;
            }
            Err(e) => {
                warn!(name = def.name, error = %e, "Failed to check system job, skipping");
                continue;
            }
            _ => {}
        }

        let payload_bytes = match def.payload.to_bytes() {
            Ok(b) => b,
            Err(e) => {
                warn!(name = def.name, error = %e, "Failed to serialize system job payload");
                continue;
            }
        };

        let result = client
            .create_job(scheduler_proto::CreateJobRequest {
                name: def.name.into(),
                owner_type: "system".into(),
                owner_id: String::new(),
                schedule: def.schedule.into(),
                payload: payload_bytes,
                workspace_id: String::new(),
                created_by: String::new(),
                conversation_id: String::new(),
            })
            .await;

        match result {
            Ok(_) => info!(name = def.name, schedule = def.schedule, "Registered system job"),
            Err(e) => warn!(name = def.name, error = %e, "Failed to register system job"),
        }
    }
}
```

Add the proto import at the top: `use crate::grpc::scheduler_proto;` (matching the existing pattern in the agent crate).

- [ ] **Step 2: Export from lib.rs**

Add `pub mod system_jobs;` to `backend/crates/sober-agent/src/lib.rs`.

- [ ] **Step 3: Verify compiles**

Run: `cd backend && cargo build -q -p sober-agent`

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-agent/src/system_jobs.rs backend/crates/sober-agent/src/lib.rs
git commit -m "feat(agent): add system job definitions and idempotent registration"
```

### Task 17: Wire everything in agent main.rs

**Files:**
- Modify: `backend/crates/sober-agent/src/main.rs` (scheduler connection at lines 158-163)

- [ ] **Step 1: Register system jobs after scheduler connects**

In the `connect_to_scheduler()` function (lines 206-267), after successfully connecting, call system job registration:

```rust
// After successful connection:
info!("Connected to scheduler, registering system jobs...");
crate::system_jobs::register_system_jobs(&client_arc).await;
```

- [ ] **Step 2: Verify compiles and starts**

Run: `cd backend && cargo build -q -p sober-agent`

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-agent/src/main.rs
git commit -m "feat(agent): wire system job registration on scheduler connect"
```

### Task 18: Clippy + cross-crate verification

- [ ] **Step 1: Run clippy across workspace**

Run: `cd backend && cargo clippy -q -- -D warnings`
Fix any warnings.

- [ ] **Step 2: Run all tests**

Run: `cd backend && cargo test --workspace -q`
Fix any failures.

- [ ] **Step 3: Regenerate sqlx offline data**

Run: `cd backend && cargo sqlx prepare --workspace -q`

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "chore: fix clippy warnings and regenerate sqlx offline data"
```

---

## Chunk 6: Integration Tests

### Task 19: Test payload serialization roundtrip

**Files:**
- Modify: `backend/crates/sober-core/src/types/job_payload.rs` (add tests)

- [ ] **Step 1: Write roundtrip tests**

Add `#[cfg(test)]` module:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_payload_roundtrip() {
        let payload = JobPayload::Prompt {
            text: "Check deploy status".into(),
            workspace_id: Some(Uuid::new_v4()),
            model_hint: None,
        };
        let bytes = payload.to_bytes().unwrap();
        let decoded = JobPayload::from_bytes(&bytes).unwrap();
        match decoded {
            JobPayload::Prompt { text, .. } => assert_eq!(text, "Check deploy status"),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_artifact_payload_roundtrip() {
        let payload = JobPayload::Artifact {
            blob_ref: "sha256:abc123".into(),
            artifact_type: ArtifactType::Wasm,
            workspace_id: Uuid::new_v4(),
            env: HashMap::from([("KEY".into(), "value".into())]),
        };
        let bytes = payload.to_bytes().unwrap();
        let decoded = JobPayload::from_bytes(&bytes).unwrap();
        match decoded {
            JobPayload::Artifact { blob_ref, .. } => assert_eq!(blob_ref, "sha256:abc123"),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_internal_payload_roundtrip() {
        let payload = JobPayload::Internal {
            operation: InternalOp::MemoryPruning,
        };
        let bytes = payload.to_bytes().unwrap();
        let decoded = JobPayload::from_bytes(&bytes).unwrap();
        assert!(matches!(
            decoded,
            JobPayload::Internal { operation: InternalOp::MemoryPruning }
        ));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cd backend && cargo test -p sober-core -q -- job_payload`
Expected: All 3 pass.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-core/
git commit -m "test(core): add JobPayload serialization roundtrip tests"
```

### Task 20: Test scheduler PauseJob/ResumeJob RPCs

**Files:**
- Add tests in: `backend/crates/sober-scheduler/src/grpc.rs` (or `tests/` directory)

- [ ] **Step 1: Write unit test for pause_job**

Test that calling `pause_job` updates the job status to `Paused` using mock repos.

- [ ] **Step 2: Write unit test for resume_job**

Test that calling `resume_job` sets status back to `Active` and recalculates `next_run_at`.

- [ ] **Step 3: Run tests**

Run: `cd backend && cargo test -p sober-scheduler -q`

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-scheduler/
git commit -m "test(scheduler): add PauseJob/ResumeJob RPC tests"
```

### Task 21: Version bump

Per CLAUDE.md, every `feat/` PR requires a MINOR version bump.

- [ ] **Step 1: Bump version in affected crate Cargo.toml files**

Bump MINOR version in:
- `backend/crates/sober-core/Cargo.toml`
- `backend/crates/sober-agent/Cargo.toml`
- `backend/crates/sober-scheduler/Cargo.toml`
- `backend/crates/sober-db/Cargo.toml`
- `backend/crates/sober-mind/Cargo.toml`

Also bump the workspace `Cargo.toml` if there is a workspace-level version.

- [ ] **Step 2: Commit**

```bash
git add backend/crates/*/Cargo.toml
git commit -m "chore: bump minor version for agent-scheduler integration"
```

### Task 22: Final verification

- [ ] **Step 1: Full workspace build**

Run: `cd backend && cargo build -q`

- [ ] **Step 2: Full workspace test**

Run: `cd backend && cargo test --workspace -q`

- [ ] **Step 3: Clippy clean**

Run: `cd backend && cargo clippy -q -- -D warnings`

- [ ] **Step 4: Regenerate sqlx offline data**

Run: `cd backend && cargo sqlx prepare --workspace -q`

- [ ] **Step 5: Final commit and summary**

```bash
git add -A
git commit -m "feat(agent-scheduler): complete agent-scheduler integration (#023)"
```
