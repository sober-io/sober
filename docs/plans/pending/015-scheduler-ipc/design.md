# 015 — Internal Scheduler & Service Communication

> Autonomous tick engine and gRPC-based inter-service communication.
> Date: 2026-03-06

---

## Overview

Sõber needs an internal scheduler that drives autonomous operations without user input,
and a secure communication layer for services to invoke each other. The scheduler runs
as an independent process alongside `sober-api`, both able to trigger the agent.

---

## 1. sober-scheduler

### Responsibilities

| Category | Examples | Tick resolution |
|----------|----------|-----------------|
| Memory maintenance | BCF compaction, importance score decay, pruning expired contexts | Minutes |
| System housekeeping | Key rotation, dead replica cleanup, health checks | Seconds-minutes |
| Proactive agent tasks | "Check X every hour", monitoring, scheduled reminders | Minutes (cron) |
| User-defined jobs | "Summarize my email every morning" | Cron expressions |
| Self-evolution | Periodic skill/plugin updates, capability assessments, soul trait evolution (via sober-mind) | Hours-daily |

### Scheduling Models

**Interval-based** -- for system tasks:
```
every: 30s   -- health checks
every: 5m    -- memory pruning scan
every: 1h    -- importance score decay
```

**Cron expressions** -- for user/agent-defined tasks:
```
"0 9 * * MON-FRI"  -- weekday morning summary
"*/15 * * * *"     -- every 15 minutes
```

Configurable minimum resolution, defaulting to minute-level. System tasks can opt
into second-level granularity.

### Persistence -- Hybrid Model

- **Ephemeral** (in-memory): system housekeeping tasks that re-register on startup.
  No DB overhead.
- **Persistent** (PostgreSQL): user-defined jobs, agent-created recurring tasks.
  Survive restarts.

```sql
CREATE TABLE scheduled_jobs (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    owner_type  TEXT NOT NULL,         -- 'system', 'user', 'agent'
    owner_id    UUID,                  -- user or agent ID (NULL for system)
    schedule    TEXT NOT NULL,          -- cron expr or interval string
    payload     JSONB NOT NULL,        -- task envelope
    next_run_at TIMESTAMPTZ NOT NULL,
    last_run_at TIMESTAMPTZ,
    status      TEXT NOT NULL DEFAULT 'active',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### Runtime Management

`soberctl scheduler` commands:
- `soberctl scheduler list` -- show all jobs with next run time
- `soberctl scheduler pause` -- pause the tick engine
- `soberctl scheduler resume` -- resume
- `soberctl scheduler run <job-id>` -- force-run a job immediately
- `soberctl scheduler cancel <job-id>` -- cancel a job

The agent can also create/cancel scheduled tasks via gRPC during conversations
("I'll check on that for you every morning").

---

## 2. Internal Service Communication

### Protocol: gRPC over Unix Domain Sockets

All inter-service communication uses gRPC (tonic + prost) over Unix domain sockets.
Proto definitions live in `shared/proto/`.

**Why gRPC/UDS:**
- Strongly typed service definitions with code generation
- Streaming support (useful for agent status updates)
- Can upgrade to TCP later for distributed deployment without protocol changes
- `tonic` + `prost` are mature in the Rust ecosystem
- Resolves the `shared/` directory purpose (proto files for internal communication)

### Service Definitions

```protobuf
// shared/proto/scheduler.proto
service SchedulerService {
    rpc CreateJob(CreateJobRequest) returns (Job);
    rpc CancelJob(CancelJobRequest) returns (Empty);
    rpc ListJobs(ListJobsRequest) returns (ListJobsResponse);
    rpc PauseScheduler(Empty) returns (Empty);
    rpc ResumeScheduler(Empty) returns (Empty);
    rpc ForceRun(ForceRunRequest) returns (Empty);
}

// shared/proto/agent.proto (agent is a gRPC server from day one — C1)
service AgentService {
    rpc ExecuteTask(TaskRequest) returns (TaskResponse);
    rpc WakeAgent(WakeRequest) returns (WakeResponse);
}

// TaskRequest includes context fields so the agent can resolve
// user identity, conversation, and workspace for prompt assembly.
// See plan.md step 1 for full proto definition.
```

### Security -- Two Layers

1. **Unix socket file permissions** (`0660`, `sober:sober`) -- prevents unauthorized
   processes from connecting.
2. **Ed25519 service identity tokens** -- each service has a keypair from `sober-crypto`,
   signs a token passed as gRPC metadata. Receiving service verifies the signature and
   checks the caller's identity against an allowlist.

Filesystem permissions are the first gate. Signed tokens identify *which* service is
calling, so the agent knows "this request came from the scheduler" vs. another process
running as the same user. Defense in depth without certificate management overhead.

For future distributed deployment, upgrade to mTLS -- the gRPC layer supports it
without protocol changes.

---

## 3. Runtime Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Clients                              │
│  PWA (Svelte)  │  Discord Bot  │  CLI (sober/soberctl)      │
└──────┬──────────────┬──────────────┬────────────────────────┘
       │              │              │
       ▼              ▼              ▼
┌─────────────────────────────────────────────────────────────┐
│                    sober-api (HTTP/WS)                       │
│         gRPC client for agent + scheduler                    │
└──────────────────────────┬──────────────────────────────────┘
                           │ gRPC/UDS
       ┌───────────────────┼───────────────────┐
       ▼                   ▼                   ▼
┌──────────────┐  ┌────────────────┐  ┌────────────────┐
│  sober-auth  │  │  sober-agent   │  │ sober-scheduler│
│  (library)   │  │  (gRPC server) │  │ (gRPC server)  │
└──────────────┘  └───────┬────────┘  └───────┬────────┘
                          │    gRPC/UDS       │
                          ◄───────────────────►
```

Independent processes:
- **sober-api** -- HTTP/WS gateway, user-driven entry point
- **sober-scheduler** -- autonomous tick engine, time-driven entry point
- **sober-agent** -- gRPC server, invoked by both API and scheduler

`soberctl` connects to API and scheduler admin sockets independently.

---

## 4. Dependency Graph

```
sober-scheduler ──► sober-core (types, config, errors)
                ──► sqlx (job persistence)
                ──► tonic (gRPC server + client)
                ──► prost (proto codegen)
                ──► sober-crypto (service identity)

sober-agent     ──► sober-core
                ──► sober-mind (prompt assembly, access masks)
                ──► tonic (gRPC server + client)
                ──► prost (proto codegen)
                ──► sober-crypto (service identity)

shared/proto/   ◄── both scheduler and agent generate from these
```

No circular crate dependencies. Scheduler and agent communicate at runtime via
gRPC, not at compile time via crate dependencies. Both generate client stubs from
the same proto definitions.

---

## 5. Impact on Existing Architecture

### New crate
- `sober-scheduler` added to workspace

### Modified crates
- `sober-agent` is already a gRPC server (decided in C1 --- agent is gRPC from day one, not converted later). This plan defines the agent's TaskRequest proto with context fields the agent needs for resolution.
- `sober-api` is already a gRPC client of the agent (established in 012). This plan adds the scheduler as an additional gRPC target for the API.
- `sober-cli` gains `soberctl scheduler` subcommands

### New shared artifacts
- `shared/proto/` contains `.proto` files for all internal service definitions
- Proto codegen integrated into build via `tonic-build` in `build.rs`

### New dependencies
- `tonic` -- gRPC framework
- `prost` -- Protocol Buffers codegen
- `tonic-build` -- build-time proto compilation
- `cron` (or similar) -- cron expression parsing for the scheduler
