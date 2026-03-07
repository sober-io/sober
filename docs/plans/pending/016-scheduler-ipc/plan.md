# 016 — sober-scheduler + Internal Service Communication: Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add an autonomous scheduler process and gRPC/UDS inter-service communication layer.

**Architecture:** Independent `sober-scheduler` runtime communicates with `sober-agent`
via gRPC over Unix domain sockets. Both generate client/server code from shared proto
definitions, avoiding circular crate dependencies. Service calls are authenticated with
filesystem permissions + Ed25519 identity tokens.

**Tech Stack:** tonic, prost, tonic-build, tokio-cron-scheduler or cron crate, sqlx, sober-crypto

**Design:** [design.md](./design.md)

**Prerequisites:** Phases 002 (skeleton), 003 (core), 004 (crypto), 012 (agent).
Phase 002 must be updated to include `sober-scheduler` stub crate and `shared/proto/` directory.

---

## Pre-work: Update Phase 002 Skeleton

Before this plan can execute, phase 002 must include:
- `sober-scheduler` stub crate in the workspace (`backend/crates/sober-scheduler/`)
- `shared/proto/` directory (replacing the `.gitkeep` placeholder)
- `tonic`, `prost`, `tonic-build` in `[workspace.dependencies]`

---

## Steps

### 1. Create proto definitions

Create `shared/proto/sober/agent/v1/agent.proto`:

```protobuf
syntax = "proto3";
package sober.agent.v1;

service AgentService {
  rpc ExecuteTask(TaskRequest) returns (TaskResponse);
  rpc WakeAgent(WakeRequest) returns (WakeResponse);
}

message TaskRequest {
  string task_id = 1;
  string task_type = 2;
  bytes payload = 3;
  string caller_identity = 4;
  // Context fields for agent resolution
  optional string user_id = 5;
  optional string conversation_id = 6;
  optional string workspace_id = 7;
}

message TaskResponse {
  string task_id = 1;
  bool success = 2;
  bytes result = 3;
  string error_message = 4;
}

message WakeRequest {
  string reason = 1;
  string caller_identity = 2;
}

message WakeResponse {
  bool accepted = 1;
}
```

Create `shared/proto/sober/scheduler/v1/scheduler.proto`:

```protobuf
syntax = "proto3";
package sober.scheduler.v1;

service SchedulerService {
  rpc CreateJob(CreateJobRequest) returns (Job);
  rpc CancelJob(CancelJobRequest) returns (CancelJobResponse);
  rpc ListJobs(ListJobsRequest) returns (ListJobsResponse);
  rpc GetJob(GetJobRequest) returns (Job);
  rpc PauseScheduler(PauseRequest) returns (PauseResponse);
  rpc ResumeScheduler(ResumeRequest) returns (ResumeResponse);
  rpc ForceRun(ForceRunRequest) returns (ForceRunResponse);
}

message Job {
  string id = 1;
  string name = 2;
  string owner_type = 3;
  optional string owner_id = 4;
  string schedule = 5;
  bytes payload = 6;
  string status = 7;
  string next_run_at = 8;
  optional string last_run_at = 9;
  string created_at = 10;
}

message CreateJobRequest {
  string name = 1;
  string owner_type = 2;
  optional string owner_id = 3;
  string schedule = 4;
  bytes payload = 5;
}

message CancelJobRequest {
  string job_id = 1;
}

message CancelJobResponse {}

message ListJobsRequest {
  optional string owner_type = 1;
  optional string owner_id = 2;
  optional string status = 3;
}

message ListJobsResponse {
  repeated Job jobs = 1;
}

message GetJobRequest {
  string job_id = 1;
}

message PauseRequest {}
message PauseResponse {}
message ResumeRequest {}
message ResumeResponse {}

message ForceRunRequest {
  string job_id = 1;
}

message ForceRunResponse {
  bool accepted = 1;
}
```

- [ ] Both proto files exist
- [ ] Proto files compile with `protoc --lint_out=. *.proto` (syntax valid)

### 2. Set up proto codegen with tonic-build

Create `backend/crates/sober-scheduler/build.rs`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &[
                "../../shared/proto/sober/scheduler/v1/scheduler.proto",
                "../../shared/proto/sober/agent/v1/agent.proto",
            ],
            &["../../shared/proto"],
        )?;
    Ok(())
}
```

Add equivalent `build.rs` to `sober-agent` for its own proto codegen (server for
agent.proto, client for scheduler.proto).

Add `tonic-build` as a build-dependency and `tonic`, `prost` as regular dependencies
to both crates.

- [ ] `cargo build -p sober-scheduler` generates proto Rust code
- [ ] `cargo build -p sober-agent` generates proto Rust code

### 3. Implement service identity authentication

Add to `sober-crypto` (or a new module within it):

- `ServiceIdentity` struct holding a service name and Ed25519 keypair
- `ServiceIdentity::generate(service_name)` --- create new identity
- `ServiceIdentity::sign_token()` --- create a signed token containing service name
  and timestamp
- `ServiceIdentity::verify_token(token, expected_service)` --- verify signature and
  check service name against allowlist

Create a tonic interceptor that:
- On client side: injects the signed token into gRPC metadata (`x-service-identity`)
- On server side: extracts and verifies the token, rejects with `UNAUTHENTICATED`
  if invalid

- [ ] Token sign/verify round-trip test passes
- [ ] Expired token (if TTL is implemented) is rejected
- [ ] Wrong service name is rejected

### 4. Define agent's TaskRequest handling for scheduler context

> **Note:** sober-agent is already a gRPC server (decided in C1 --- agent is gRPC
> from day one, implemented in 012). This step does NOT create the gRPC server
> scaffolding --- that already exists. Instead, it ensures the agent's
> `ExecuteTask` handler correctly unpacks the context fields (`user_id`,
> `conversation_id`, `workspace_id`) from `TaskRequest` and uses them to resolve
> prompt context via sober-mind.

Verify/update `AgentServiceImpl::execute_task`:

- Extract optional `user_id`, `conversation_id`, `workspace_id` from `TaskRequest`
- Build a `PromptContext` (from sober-mind) using these fields
- Pass the context to the agent's task execution logic

- [ ] Agent resolves user/workspace context from TaskRequest fields
- [ ] Missing context fields (e.g., system tasks with no user) are handled gracefully

### 5. Create sober-scheduler module structure

```
sober-scheduler/src/
  main.rs          -- entry point, startup, shutdown
  config.rs        -- scheduler configuration
  engine.rs        -- tick engine loop
  job.rs           -- job types, scheduling logic
  store.rs         -- PostgreSQL persistence for jobs
  grpc/
    mod.rs         -- gRPC server setup
    scheduler_service.rs -- SchedulerService implementation
  admin.rs         -- admin Unix socket
```

### 6. Implement job types and scheduling logic

In `job.rs`:

- `JobSchedule` enum: `Interval(Duration)` and `Cron(CronExpression)`
- `JobOwner` enum: `System`, `User(Uuid)`, `Agent(Uuid)`
- `Job` struct: id, name, owner, schedule, payload, status, next_run_at, last_run_at
- `Job::next_run(&self, now) -> Option<DateTime<Utc>>` --- calculate next execution
  time based on schedule type
- `JobStatus` enum: `Active`, `Paused`, `Cancelled`, `Running`

Use the `cron` crate for cron expression parsing.

- [ ] Interval schedule correctly calculates next run
- [ ] Cron expression parsing works for standard expressions
- [ ] Invalid cron expressions return an error

### 7. Implement job persistence (store.rs)

- `JobStore::new(pool)` --- takes a PgPool
- `JobStore::create(job) -> Result<Job>`
- `JobStore::get(id) -> Result<Job>`
- `JobStore::list(filter) -> Result<Vec<Job>>`
- `JobStore::update_next_run(id, next_run_at)`
- `JobStore::update_status(id, status)`
- `JobStore::cancel(id)`
- `JobStore::due_jobs(now) -> Result<Vec<Job>>` --- find all active jobs where
  `next_run_at <= now`

SQL migration for the `scheduled_jobs` table (see design.md for schema).

- [ ] CRUD operations work against test database
- [ ] `due_jobs` correctly returns only jobs past their next_run_at

### 8. Implement the tick engine (engine.rs)

- `TickEngine::new(config, job_store, agent_client)` --- takes store and gRPC client
  for agent
- `TickEngine::register_system_job(name, schedule, handler)` --- register ephemeral
  system tasks
- `TickEngine::run(cancel_token)` --- main loop:
  1. Sleep until next tick (configurable interval, default 1 second)
  2. Query `due_jobs(now)` from store
  3. Collect due system jobs from in-memory registry
  4. For each due job:
     a. Mark as `Running`
     b. Spawn task to execute (call agent via gRPC or run system handler)
     c. On completion: update `last_run_at`, calculate and set `next_run_at`,
        set status back to `Active`
     d. On failure: log error, set status back to `Active` (retry on next tick)
  5. Repeat
- `TickEngine::pause()` / `resume()` --- stop/start the tick loop

- [ ] Engine runs and picks up due jobs
- [ ] Pausing stops job execution, resuming restarts it
- [ ] System jobs registered in memory are executed
- [ ] Persistent jobs from DB are executed

### 9. Implement SchedulerService gRPC server

In `grpc/scheduler_service.rs`:

- `SchedulerServiceImpl` holding `JobStore` and `TickEngine` handle
- Implement all RPCs: `CreateJob`, `CancelJob`, `ListJobs`, `GetJob`,
  `PauseScheduler`, `ResumeScheduler`, `ForceRun`
- Apply service identity interceptor (only accept calls from known services)

In `grpc/mod.rs`:

- `start_grpc_server(socket_path, service)` --- bind to Unix socket, serve

- [ ] `CreateJob` creates a persistent job and returns it
- [ ] `ListJobs` returns filtered results
- [ ] `PauseScheduler` / `ResumeScheduler` control the tick engine
- [ ] `ForceRun` immediately executes a job

### 10. Implement scheduler main.rs

Startup sequence:
1. Load `AppConfig` from env (uses `SchedulerConfig` section for tick interval,
   socket paths, max concurrent jobs; `DatabaseConfig` for DB URL)
2. Initialize tracing
3. Connect to PostgreSQL
4. Run pending migrations
5. Create `JobStore`
6. Create agent gRPC client (connect to agent's UDS)
7. Create `TickEngine`
8. Register default system jobs (health check, etc.)
9. Start gRPC server on scheduler's UDS
10. Start admin socket (optional, for `soberctl`)
11. Start tick engine
12. Wait for shutdown signal (SIGTERM/SIGINT)
13. Graceful shutdown: stop tick engine, wait for running jobs, close connections

- [ ] Process starts, connects to DB, binds sockets
- [ ] Graceful shutdown completes cleanly

### 11. Add agent-side gRPC client for scheduler

In `sober-agent`, add a gRPC client module for calling the scheduler:

- `SchedulerClient::connect(socket_path)` --- connect to scheduler UDS
- `SchedulerClient::create_job(request)` --- create a scheduled job
- `SchedulerClient::cancel_job(job_id)` --- cancel a job

This allows the agent to say "I'll check on that every morning" and create a
scheduled job via gRPC.

- [ ] Agent can create a job on the scheduler
- [ ] Agent can cancel a job on the scheduler

### 12. Add sober-api gRPC client for scheduler

> **Note:** sober-api already uses a gRPC client to invoke the agent (established
> in 013). This step adds the scheduler as an additional gRPC target.

Add `SchedulerClient::connect(socket_path)` to sober-api for scheduler management.

- [ ] API server can list/create/cancel scheduled jobs via gRPC to scheduler

### 13. Add soberctl scheduler commands

Add `soberctl scheduler` subcommand group to `sober-cli`:

- `soberctl scheduler list [--status active|paused|cancelled]`
- `soberctl scheduler pause`
- `soberctl scheduler resume`
- `soberctl scheduler run <job-id>`
- `soberctl scheduler cancel <job-id>`

These connect to the scheduler's admin socket and call the SchedulerService RPCs.

- [ ] `soberctl scheduler list` displays jobs in a table
- [ ] `soberctl scheduler pause` / `resume` controls the engine
- [ ] `soberctl scheduler run` force-executes a job

### 14. Write integration tests

- Scheduler starts, registers a system job, job executes on schedule
- Persistent job created via gRPC, survives scheduler restart
- Agent creates a scheduled job via gRPC, scheduler executes it and calls
  agent back
- `ForceRun` immediately triggers a job
- Pause/resume stops and restarts job execution
- Service identity: unauthenticated caller is rejected
- Service identity: wrong service name is rejected

- [ ] All integration tests pass

### 15. Update docker-compose.yml

Add scheduler process to the Docker Compose configuration. It needs:
- PostgreSQL access (same as API)
- Unix socket volume shared with API and agent for gRPC communication
- Environment variables for socket paths

- [ ] `docker compose up` starts scheduler alongside API and agent

### 16. Lint and test

Run `cargo clippy -p sober-scheduler -- -D warnings` and
`cargo test -p sober-scheduler`. Also run workspace-wide tests to verify
no regressions in other crates.

- [ ] No clippy warnings
- [ ] All tests pass

---

## Acceptance Criteria

- Scheduler process starts independently and runs a tick loop.
- System tasks (ephemeral) register at startup and execute on schedule.
- Persistent jobs survive scheduler restarts (stored in PostgreSQL).
- Interval-based and cron-based scheduling both work correctly.
- Agent and scheduler communicate via gRPC over Unix domain sockets.
- Service identity tokens are verified on all gRPC calls.
- Unauthorized callers are rejected with `UNAUTHENTICATED`.
- Agent can create/cancel scheduled jobs via gRPC.
- `soberctl scheduler` commands work for admin management.
- Graceful shutdown waits for running jobs to complete.
- `cargo clippy -- -D warnings` passes for all affected crates.
- `cargo test --workspace` passes with all new tests green.
