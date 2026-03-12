# Agent–Scheduler Integration Design

> **Status:** Draft
> **Date:** 2026-03-12
> **Scope:** Backend integration only (no REST API, no frontend UI)

## Goal

Wire up proper integration between `sober-agent`, `sober-scheduler`, and `sober-workspace`
so that:

1. Users can create and manage scheduled jobs through conversation with the agent.
2. Jobs execute autonomously (no active conversation required).
3. Jobs are scoped to workspaces (user or group).
4. Job results are stored as workspace artifacts.
5. System-level maintenance and self-evolution jobs run automatically.

## Approach: Job-Type-Aware Agent

The scheduler remains a simple tick engine. The agent becomes the single orchestrator
for all job execution — it receives jobs from the scheduler, determines the execution
path (LLM prompt, sandboxed artifact, or internal operation), executes, and stores results.

```
Scheduler (tick fires)
  → queries due jobs from PostgreSQL
  → calls Agent.ExecuteTask() via gRPC/UDS (forwarding workspace_id from Job)
  → Agent parses typed payload
    ├── Prompt job → sober-mind prompt assembly → LLM → result
    ├── Artifact job → sober-sandbox execution → result
    └── Internal job → direct crate method call → result
  → Agent stores result as workspace artifact (or in job_runs.result for system jobs)
  → Scheduler records job_run with artifact reference
```

---

## 1. Job Payload Model

A discriminated payload enum that determines the agent's execution path.
Serialized with `bincode` into the existing `payload_bytes BYTEA` column.

The existing `payload` JSON column (`serde_json::Value`) becomes vestigial for
bincode-encoded payloads. The scheduler's `create_job` handler already writes
`{"raw": true}` as a fallback when bytes aren't valid JSON — this continues
to work. All payload logic uses `payload_bytes`; the JSON `payload` column is
retained for backward compatibility but not used for typed jobs.

```rust
// sober-core/src/types/domain.rs

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
    Internal {
        operation: InternalOp,
    },
}

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

**Result storage:** Job outputs are stored as workspace artifacts via
`sober-workspace::blob::store()`. The `job_runs` table gains a `result_artifact_ref`
column (nullable `TEXT`) pointing to the content-addressed blob. System jobs without
a workspace store results in the job_runs `result` field directly.

---

## 2. Workspace & Owner Scoping

### Owner types

| owner_type | owner_id | Meaning |
|------------|----------|---------|
| `system` | NULL | System maintenance/evolution jobs. Admin-only via soberctl. |
| `user` | user UUID | Personal jobs. Only the owner can manage them. |
| `group` | group UUID | Group workspace jobs. Any member creates; members manage their own, admins manage all. |

The existing `agent` owner type is dropped — agent-created jobs are owned by the
user or group they serve. Existing rows with `owner_type = 'agent'` are migrated
to `'system'` (see Section 6 migration).

### New fields on `Job`

Added to `Job` domain type in `sober-core`, `CreateJob` input type, `PgJobRepo::create()`,
and the scheduler proto:

- `workspace_id: Option<Uuid>` — which workspace context to use for execution.
  System jobs may be NULL.
- `created_by: Option<Uuid>` — the user who created the job. NULL for system jobs.
  Important for group jobs where `owner_id` is the group UUID, not the individual.

### Authorization rules

Enforced in the agent's `SchedulerTools` before any scheduler gRPC call.

Group membership and admin checks are resolved by querying the `RoleRepo` / group
membership tables in `sober-db`. The `SchedulerTools` struct holds a reference to
the relevant repo (or a lightweight authorization service) alongside the scheduler client.

```rust
pub struct SchedulerTools {
    scheduler_client: SharedSchedulerClient,
    auth: Arc<dyn AuthorizationService>,  // checks group membership, admin status
}
```

Authorization rules:

- **User jobs:** `created_by == caller.user_id` required for all operations.
- **Group jobs (view/create):** caller must be a member of the group
  (checked via `auth.is_member_of(caller.user_id, group_id)`).
- **Group jobs (modify/cancel):** `created_by == caller.user_id` OR
  `auth.is_admin_of(caller.user_id, group_id)`.
- **System jobs:** admin only, via `soberctl`. Never manageable through conversation.

---

## 3. Autonomous Agent Execution

The agent's `execute_task()` gRPC method is reimplemented to support autonomous
(non-conversational) execution.

> **Note:** Code below is pseudocode for clarity. The actual implementation uses
> `mpsc::channel` + `tokio::spawn` to produce a `ReceiverStream`, matching the
> existing `execute_task()` pattern in the agent crate.

```rust
// sober-agent/src/grpc.rs — execute_task() reimplemented (pseudocode)

async fn execute_task(&self, request: ExecuteTaskRequest) {
    let payload: JobPayload = bincode::deserialize(&request.payload)?;

    match payload {
        JobPayload::Prompt { text, workspace_id, model_hint } => {
            // Load workspace context (SOUL.md, config, relevant memory)
            let context = match workspace_id {
                Some(ws_id) => self.load_workspace_context(ws_id).await?,
                None => self.load_system_context().await?,
            };

            // Assemble prompt via sober-mind
            // Uses CallerContext with TriggerKind::Scheduler for full internal access
            let caller = CallerContext::scheduler(request.user_id);
            let prompt = self.mind.assemble_autonomous_prompt(
                &context, &text, &caller,
            )?;

            // Execute via LLM
            let response = self.llm.complete(prompt, model_hint).await?;

            // Store result as workspace artifact (if workspace present)
            let artifact_ref = if let Some(ws_id) = workspace_id {
                Some(self.workspace.store_artifact(ws_id, "job-result", &response).await?)
            } else {
                None
            };

            // Send result via channel
            tx.send(AgentEvent::TextDelta(response.summary())).await;
            tx.send(AgentEvent::Done { message_id, usage, artifact_ref }).await;
        }

        JobPayload::Artifact { blob_ref, artifact_type, workspace_id, env } => {
            // Resolve artifact from workspace blob store
            let blob_path = self.workspace.blob_resolve_to_path(workspace_id, &blob_ref).await?;

            // Build sandbox command based on artifact type
            let command = match artifact_type {
                ArtifactType::Wasm => vec!["wasmtime".into(), "run".into(), blob_path],
                ArtifactType::Script => vec![blob_path],  // must be executable
            };

            // Build sandbox policy
            let policy = SandboxPolicy::for_scheduled_job(&artifact_type, &workspace_id);

            // Execute in sandbox — BwrapSandbox::execute takes command array + env
            let result = self.sandbox.execute(&command, &policy, &env).await?;

            // Store result as workspace artifact
            let artifact_ref = self.workspace.store_artifact(
                workspace_id, "job-result", &result.stdout,
            ).await?;

            if result.exit_code != 0 {
                tx.send(AgentEvent::Error(result.stderr)).await;
            }
            tx.send(AgentEvent::Done { message_id, usage, artifact_ref }).await;
        }

        JobPayload::Internal { operation } => {
            // Dispatch directly to the appropriate crate method — no LLM
            let result = match operation {
                InternalOp::MemoryPruning => self.memory.prune_expired().await?,
                InternalOp::SessionCleanup => self.db.cleanup_sessions().await?,
                InternalOp::VectorIndexOptimize => self.memory.optimize_indices().await?,
                InternalOp::PluginAudit => self.plugin.audit_installed().await?,
            };

            tx.send(AgentEvent::TextDelta(result.summary())).await;
            tx.send(AgentEvent::Done { message_id, usage: Usage::zero(), artifact_ref: None }).await;
        }
    }
}
```

### Proto changes for `AgentEvent::Done`

The existing `Done` message in `agent.proto` gains an optional `artifact_ref` field:

```protobuf
message Done {
  string message_id = 1;
  uint32 prompt_tokens = 2;
  uint32 completion_tokens = 3;
  string artifact_ref = 4;  // NEW — optional, content-addressed blob ref
}
```

The Rust `AgentEvent::Done` enum variant is updated to include `artifact_ref: Option<String>`.

### New sober-mind method

```rust
/// Assemble a prompt for autonomous (non-conversational) execution.
/// Loads SOUL.md chain, workspace context, relevant memory — no conversation history.
/// Uses CallerContext (with TriggerKind::Scheduler) for access control,
/// matching the existing Mind::assemble() pattern.
pub fn assemble_autonomous_prompt(
    &self,
    context: &WorkspaceContext,
    task: &str,
    caller: &CallerContext,
) -> Result<Prompt, MindError>;
```

### Execution context by caller

| Caller | Context loaded | CallerContext trigger |
|--------|---------------|----------------------|
| Scheduler (system job) | System SOUL.md, global memory | `TriggerKind::Scheduler` |
| Scheduler (user/group job) | Workspace SOUL.md chain, workspace memory | `TriggerKind::Scheduler` |
| User conversation | Conversation history, user memory | `TriggerKind::Human` |

### Scheduler engine change

The existing `execute_via_agent()` in `sober-scheduler/src/engine.rs` currently
sends `workspace_id: None`. This must be updated to read `workspace_id` from the
`Job` domain type and forward it:

```rust
// engine.rs — execute_via_agent() update
let request = ExecuteTaskRequest {
    task_id: job.id.to_string(),
    task_type: "scheduled_job".into(),
    payload: job.payload_bytes.clone(),
    caller_identity: "scheduler".into(),
    user_id: job.owner_id.map(|id| id.to_string()).unwrap_or_default(),
    conversation_id: String::new(),
    workspace_id: job.workspace_id.map(|id| id.to_string()).unwrap_or_default(), // NEW
};
```

---

## 4. Agent Scheduler Tools (Conversational Job Management)

The agent exposes scheduling as tools the LLM can call during user conversations.

### Tool definitions

| Tool | Parameters | Description |
|------|-----------|-------------|
| `create_scheduled_job` | name, schedule, job_type (prompt\|artifact), content, workspace_id? | Create a new job |
| `list_scheduled_jobs` | workspace_id?, status? | List jobs visible to caller |
| `get_scheduled_job` | job_id | Get job details + recent runs |
| `cancel_scheduled_job` | job_id | Cancel a job permanently |
| `pause_scheduled_job` | job_id | Pause (can resume later) |
| `resume_scheduled_job` | job_id | Resume a paused job |
| `get_job_runs` | job_id, limit? | View execution history |

### Per-job pause/resume

The existing scheduler proto only has `PauseScheduler`/`ResumeScheduler` (global).
Per-job pause/resume requires new RPCs:

```protobuf
// scheduler.proto — new RPCs
service SchedulerService {
  // ... existing RPCs ...
  rpc PauseJob(PauseJobRequest) returns (PauseJobResponse);
  rpc ResumeJob(ResumeJobRequest) returns (ResumeJobResponse);
}

message PauseJobRequest { string job_id = 1; }
message PauseJobResponse { Job job = 1; }
message ResumeJobRequest { string job_id = 1; }
message ResumeJobResponse { Job job = 1; }
```

Implementation: call `JobRepo::update_status(job_id, JobStatus::Paused)` /
`JobRepo::update_status(job_id, JobStatus::Active)` + recalculate `next_run_at`
on resume.

### Implementation

A `SchedulerTools` struct in `sober-agent/src/tools/scheduler.rs` wraps the
`SharedSchedulerClient` and an authorization service, implementing each tool
with authorization checks.

```rust
pub struct SchedulerTools {
    scheduler_client: SharedSchedulerClient,
    auth: Arc<dyn AuthorizationService>,
}

impl SchedulerTools {
    async fn authorize(
        &self,
        caller_user_id: Uuid,
        job: &Job,
        action: JobAction,
    ) -> Result<(), AppError> {
        match (job.owner_type.as_str(), action) {
            ("user", _) => {
                ensure!(job.created_by == Some(caller_user_id), AppError::Forbidden);
            }
            ("group", JobAction::View | JobAction::Create) => {
                ensure!(
                    self.auth.is_member_of(caller_user_id, job.owner_id.unwrap()).await?,
                    AppError::Forbidden,
                );
            }
            ("group", _) => {
                let is_creator = job.created_by == Some(caller_user_id);
                let is_admin = self.auth.is_admin_of(caller_user_id, job.owner_id.unwrap()).await?;
                ensure!(is_creator || is_admin, AppError::Forbidden);
            }
            ("system", _) => return Err(AppError::Forbidden),
            _ => return Err(AppError::Forbidden),
        }
        Ok(())
    }

    pub async fn create_job(
        &self,
        params: CreateJobParams,
        caller_user_id: Uuid,
    ) -> ToolResult {
        // Validate schedule format
        let _ = JobSchedule::parse(&params.schedule)?;

        let payload = match params.job_type {
            JobType::Prompt => JobPayload::Prompt {
                text: params.content,
                workspace_id: params.workspace_id,
                model_hint: None,
            },
            JobType::Artifact => JobPayload::Artifact {
                blob_ref: params.content,
                artifact_type: ArtifactType::Wasm,
                workspace_id: params.workspace_id.expect("artifact jobs require workspace"),
                env: HashMap::new(),
            },
        };

        let req = CreateJobRequest {
            name: params.name,
            owner_type: determine_owner_type(&params),
            owner_id: caller_user_id.to_string(),
            schedule: params.schedule,
            payload: bincode::serialize(&payload)?,
            workspace_id: params.workspace_id.map(|id| id.to_string()).unwrap_or_default(),
            created_by: caller_user_id.to_string(),
        };

        let client = self.scheduler_client.read().await;
        let job = client.as_ref().unwrap().create_job(req).await?;

        ToolResult::success(format!(
            "Created job '{}' ({}). Next run: {}",
            job.name, job.id, job.next_run_at
        ))
    }
}
```

### Conversation examples

```
User: "Run a deploy check every 30 minutes in this workspace"
Agent: → create_scheduled_job(name: "Deploy check", schedule: "every: 30m",
         job_type: "prompt", content: "Check deployment status and report issues")
Agent: "I've scheduled 'Deploy check' every 30 minutes. Next run at 14:30 UTC."

User: "What jobs do I have running?"
Agent: → list_scheduled_jobs()
Agent: "You have 2 active jobs:
        1. Deploy check — every 30m — last ran 14:00, succeeded
        2. Daily inbox summary — 0 9 * * * — last ran 09:00, succeeded"

User: "Pause the deploy check"
Agent: → pause_scheduled_job(job_id: "...")
Agent: "Paused 'Deploy check'. Say 'resume' when you want it running again."
```

---

## 5. Job Result Delivery to Conversations

Scheduled jobs must not be silent — users need feedback. When a job completes,
its results are delivered back to a conversation so the user sees them in their
chat history.

### Conversation linking

Jobs track which conversation they were created from:

- `conversation_id: Option<Uuid>` — the conversation where the job was created
  (via agent tool call). NULL for system jobs and jobs created via `soberctl`.

**Resolution strategy** for where to deliver results:

| Scenario | Delivery target |
|----------|----------------|
| Job has `conversation_id` and that conversation still exists | Original conversation |
| Job has `conversation_id` but conversation was deleted | User's most recent conversation in the same workspace |
| Job has no `conversation_id` (soberctl-created user job) | User's most recent conversation in the job's workspace |
| System job | No conversation delivery — results in `job_runs` only |

### Delivery mechanism

The existing WebSocket + gRPC streaming infrastructure handles everything.
When the scheduler calls `Agent.ExecuteTask()`, it passes `conversation_id`
from the job. The agent streams `AgentEvent`s back, and the API's WebSocket
handler pushes them to the connected client.

```
Scheduler tick fires
  → ExecuteTaskRequest { conversation_id: job.conversation_id, user_id, ... }
  → Agent executes job, streams AgentEvents
  → API receives stream on behalf of conversation
  → If user is connected via WebSocket: real-time push (chat.delta, chat.done)
  → If user is offline: messages stored in DB, visible when they reconnect
```

**Offline delivery:** The agent stores the assistant message in the messages
table (linked to `conversation_id`) regardless of whether the user is connected.
When the user opens that conversation, they see the job result as a regular
assistant message — no special UI needed.

### Message format in conversation

Job results appear as normal assistant messages with a job context indicator:

```
[Scheduled job: "Deploy check" — every 30m]

Deploy status looks healthy:
- Production: 3 pods running, 0 restarts in last 30m
- Staging: 2 pods running, 1 restart (recovered)
- No alerts triggered.

Result stored as artifact: sha256:abc123...
```

The agent prepends a brief header identifying the scheduled job so users can
distinguish job results from interactive responses.

### Proto & schema changes

**Jobs table** — add `conversation_id`:
```sql
ALTER TABLE jobs ADD COLUMN conversation_id UUID REFERENCES conversations(id) ON DELETE SET NULL;
```

`ON DELETE SET NULL` ensures that deleting a conversation doesn't cascade to the
job — the job continues running, and results fall back to the user's most recent
conversation (per resolution strategy above).

**Scheduler proto** — add to `Job` and `CreateJobRequest`:
```protobuf
message Job {
  // ... existing + previously proposed fields ...
  string conversation_id = 13;  // NEW — optional, conversation to deliver results to
}

message CreateJobRequest {
  // ... existing + previously proposed fields ...
  string conversation_id = 8;   // NEW
}
```

**Scheduler engine** — `execute_via_agent()` forwards `conversation_id`:
```rust
let request = ExecuteTaskRequest {
    // ... existing fields ...
    conversation_id: job.conversation_id.map(|id| id.to_string()).unwrap_or_default(),
};
```

### Fallback resolution in agent

When `execute_task()` receives a job with a `conversation_id`, the agent verifies
the conversation still exists. If not, it resolves a fallback:

```rust
// sober-agent — resolve delivery conversation (pseudocode)
async fn resolve_delivery_conversation(
    &self,
    job_conversation_id: Option<Uuid>,
    user_id: Uuid,
    workspace_id: Option<Uuid>,
) -> Option<ConversationId> {
    // Try the original conversation first
    if let Some(cid) = job_conversation_id {
        if self.conversation_repo.exists(cid).await? {
            return Some(cid);
        }
    }
    // Fallback: user's most recent conversation in the same workspace
    self.conversation_repo
        .find_latest_by_user_and_workspace(user_id, workspace_id)
        .await?
}
```

---

## 6. System-Level Jobs

Predefined jobs registered idempotently on agent startup. Managed exclusively
via `soberctl` — never through conversation.

### Predefined system jobs

| Job | Schedule | Payload type | Purpose |
|-----|----------|-------------|---------|
| `memory_pruning` | `every: 1h` | `Internal(MemoryPruning)` | Prune expired memory chunks, decay importance scores |
| `session_cleanup` | `every: 6h` | `Internal(SessionCleanup)` | Clean up expired sessions and temporary data |
| `trait_evolution_check` | `0 3 * * *` | `Prompt` | Review interaction patterns, propose SOUL.md adjustments |
| `plugin_audit` | `0 4 * * MON` | `Internal(PluginAudit)` | Audit installed plugins for security issues and updates |
| `vector_index_optimize` | `0 2 * * SUN` | `Internal(VectorIndexOptimize)` | Optimize Qdrant indices, rebalance collections |

Deterministic maintenance tasks use `JobPayload::Internal` — no LLM call, no API
cost. Only `trait_evolution_check` uses `JobPayload::Prompt` because it requires
LLM reasoning to analyze patterns and propose changes.

### Registration flow

Uses `ListJobs` with `owner_type: "system"` filter to check for existing jobs,
since `find_by_name` does not exist. A new `name` filter is added to
`ListJobsRequest` (see Section 7 proto changes).

```rust
async fn register_system_jobs(scheduler: &SchedulerClient) {
    let system_jobs = vec![
        SystemJobDef {
            name: "memory_pruning",
            schedule: "every: 1h",
            payload: JobPayload::Internal { operation: InternalOp::MemoryPruning },
        },
        SystemJobDef {
            name: "trait_evolution_check",
            schedule: "0 3 * * *",
            payload: JobPayload::Prompt {
                text: "Review interaction patterns across users. Propose SOUL.md \
                       trait adjustments if high-confidence patterns detected.".into(),
                workspace_id: None,
                model_hint: None,
            },
        },
        // ... additional jobs
    ];

    for def in system_jobs {
        // Idempotent: check if job with this name already exists for system owner
        let existing = scheduler.list_jobs(ListJobsRequest {
            owner_type: "system".into(),
            name_filter: def.name.clone(),
            ..Default::default()
        }).await?;

        if existing.jobs.is_empty() {
            scheduler.create_job(CreateJobRequest {
                name: def.name,
                owner_type: "system".into(),
                owner_id: String::new(),
                schedule: def.schedule,
                payload: bincode::serialize(&def.payload)?,
                workspace_id: String::new(),
                created_by: String::new(),
            }).await?;
        }
    }
}
```

### Execution context

- No workspace — uses system-level SOUL.md and global memory only.
- `CallerContext` with `TriggerKind::Scheduler` — full access to internal operations.
- Results stored in `job_runs.result` directly (no workspace artifact).

---

## 7. Proto & Database Changes

### Proto changes (`scheduler.proto`)

```protobuf
message Job {
  // ... existing fields (1-10, where 10 = created_at) ...
  string workspace_id = 11;   // optional, UUID as string
  string created_by = 12;     // optional, UUID of creating user
}

message CreateJobRequest {
  // ... existing fields ...
  string workspace_id = 6;
  string created_by = 7;
}

message ListJobsRequest {
  // ... existing fields ...
  string workspace_id = 4;    // filter by workspace
  string name_filter = 5;     // filter by exact name (for idempotent registration)
}

// NEW RPCs for per-job pause/resume
message PauseJobRequest { string job_id = 1; }
message PauseJobResponse { Job job = 1; }
message ResumeJobRequest { string job_id = 1; }
message ResumeJobResponse { Job job = 1; }

service SchedulerService {
  // ... existing RPCs ...
  rpc PauseJob(PauseJobRequest) returns (PauseJobResponse);
  rpc ResumeJob(ResumeJobRequest) returns (ResumeJobResponse);
}
```

### Proto changes (`agent.proto`)

```protobuf
message Done {
  string message_id = 1;
  uint32 prompt_tokens = 2;
  uint32 completion_tokens = 3;
  string artifact_ref = 4;    // NEW — optional, content-addressed blob ref for job results
}
```

### Database migration

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

### Crate dependency changes

| Crate | New dependency | Reason |
|-------|---------------|--------|
| `sober-agent` | `sober-sandbox` | Artifact job execution |
| `sober-agent` | `bincode` | JobPayload deserialization |
| `sober-core` | `bincode` | JobPayload serialization |
| `sober-scheduler` | (none) | Proto regen only |

---

## 8. Component Change Summary

| Component | Changes |
|-----------|---------|
| **sober-core** | `JobPayload` enum (Prompt/Artifact/Internal), `ArtifactType` enum, `InternalOp` enum; extended `Job` domain type with `workspace_id` + `created_by` + `conversation_id`; extended `CreateJob` input type with same fields |
| **sober-agent** | Full `execute_task()` implementation with payload dispatch (channel+spawn pattern); `SchedulerTools` struct for conversational job management with `AuthorizationService`; system job registration on startup; `AgentEvent::Done` gains `artifact_ref`; conversation resolution for job result delivery (fallback to latest conversation) |
| **sober-scheduler** | Proto + DB schema updates; `execute_via_agent()` forwards `workspace_id` + `conversation_id` from Job; new `PauseJob`/`ResumeJob` RPCs; `list_filtered` supports `name_filter`. No other execution logic changes. |
| **sober-db** | Updated `PgJobRepo::create()` and `PgJobRepo::list_filtered()` for new fields; `PgJobRunRepo::complete()` accepts `result_artifact_ref`; new `ConversationRepo::find_latest_by_user_and_workspace()`; new migration |
| **sober-mind** | New `assemble_autonomous_prompt()` method taking `CallerContext` (not `AccessMask`) for non-conversational execution |
| **Proto** | `scheduler.proto`: `workspace_id`, `created_by`, `conversation_id` on Job/CreateJobRequest; `name_filter` on ListJobsRequest; `PauseJob`/`ResumeJob` RPCs. `agent.proto`: `artifact_ref` on Done message. |

## 9. What's NOT in This Design

- REST API endpoints for job management (future work)
- Frontend UI for job management (future work)
- Chat integration beyond tool calls (e.g., natural language job DSL)
- Job retry policies or exponential backoff
- Multi-machine distributed scheduling
