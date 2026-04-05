//! System job definitions and idempotent registration.
//!
//! System jobs are maintenance tasks that the scheduler registers on startup.
//! Internal ops (memory pruning, session cleanup) execute locally via the
//! executor registry. Prompt jobs (trait evolution) are dispatched to the agent.

use chrono::Utc;
use sober_core::error::AppError;
use sober_core::types::CreateJob;
use sober_core::types::repo::JobRepo;
use tracing::{info, warn};

use crate::job::JobSchedule;

/// Definition of a built-in system job.
struct SystemJobDef {
    name: &'static str,
    schedule: &'static str,
    payload: serde_json::Value,
}

/// All built-in system jobs.
///
/// TODO: add these once their executor implementations exist:
/// - `plugin_audit` (weekly) — requires `sober-plugin` audit API
/// - `vector_index_optimize` (weekly) — requires Qdrant collection optimize API
///
/// TODO: schedules are hardcoded — make configurable via `AppConfig` or admin API
fn system_jobs() -> Vec<SystemJobDef> {
    vec![
        SystemJobDef {
            name: "system::memory_pruning",
            schedule: "every: 1h",
            payload: serde_json::json!({
                "type": "internal",
                "op": "memory_pruning",
            }),
        },
        SystemJobDef {
            name: "system::session_cleanup",
            schedule: "every: 15m",
            payload: serde_json::json!({
                "type": "internal",
                "op": "session_cleanup",
            }),
        },
        SystemJobDef {
            name: "system::plugin_cleanup",
            schedule: "every: 1h",
            payload: serde_json::json!({
                "type": "internal",
                "op": "plugin_cleanup",
            }),
        },
        SystemJobDef {
            name: crate::executors::attachment_cleanup::JOB_NAME,
            schedule: "every: 1h",
            payload: serde_json::json!({
                "type": "internal",
                "op": crate::executors::attachment_cleanup::OP,
            }),
        },
        SystemJobDef {
            name: crate::executors::blob_gc::JOB_NAME,
            schedule: "0 0 0 * * * *",
            payload: serde_json::json!({
                "type": "internal",
                "op": crate::executors::blob_gc::OP,
            }),
        },
        SystemJobDef {
            name: "system::memory_dedup",
            schedule: "0 0 2 * * * *",
            payload: serde_json::json!({
                "type": "internal",
                "op": crate::executors::memory_dedup::OP,
            }),
        },
        SystemJobDef {
            name: "system::memory_orphan_cleanup",
            schedule: "0 0 4 * * * *",
            payload: serde_json::json!({
                "type": "internal",
                "op": crate::executors::memory_orphan_cleanup::OP,
            }),
        },
        SystemJobDef {
            name: "system::trait_evolution_check",
            schedule: "0 0 3 * * * *",
            payload: serde_json::json!({
                "type": "prompt",
                "text": "Review interaction patterns across users. Propose SOUL.md \
                         trait adjustments if high-confidence patterns detected.",
            }),
        },
    ]
}

/// Register all built-in system jobs, skipping any that already exist.
///
/// Called once at scheduler startup. Jobs are identified by name — if a job
/// with the same name already exists (any status), registration is skipped.
/// After registration, emits gauge metrics for the total job counts.
pub async fn register_system_jobs<J: JobRepo>(job_repo: &J) -> Result<(), AppError> {
    // Fetch existing system jobs to check for duplicates.
    let existing = job_repo
        .list_filtered(Some("system"), None, &[], None, None, None)
        .await?;

    for def in system_jobs() {
        if existing.iter().any(|j| j.name == def.name) {
            info!(name = def.name, "system job already registered, skipping");
            continue;
        }

        let schedule = match JobSchedule::parse(def.schedule) {
            Ok(s) => s,
            Err(e) => {
                warn!(name = def.name, error = %e, "invalid system job schedule, skipping");
                continue;
            }
        };

        let now = Utc::now();
        let next_run_at = schedule
            .next_run_after(now)
            .expect("system job schedule always has a next run");

        let input = CreateJob {
            name: def.name.to_owned(),
            schedule: def.schedule.to_owned(),
            payload: def.payload,
            owner_type: "system".to_owned(),
            owner_id: None,
            workspace_id: None,
            created_by: None,
            conversation_id: None,
            next_run_at,
        };

        job_repo.create(input).await?;
        info!(
            name = def.name,
            schedule = def.schedule,
            "registered system job"
        );
    }

    // Emit gauge metrics for registered jobs.
    emit_jobs_registered_gauge(job_repo).await;

    Ok(())
}

/// Query all active/paused jobs and emit the `sober_scheduler_jobs_registered` gauge.
///
/// Categorizes jobs by schedule type (interval/cron). All jobs are persistent
/// (DB-backed), so the persistence label is always "persistent".
pub async fn emit_jobs_registered_gauge<J: JobRepo>(job_repo: &J) {
    let statuses = ["active".to_owned(), "paused".to_owned()];
    let all_jobs = match job_repo
        .list_filtered(None, None, &statuses, None, None, None)
        .await
    {
        Ok(jobs) => jobs,
        Err(e) => {
            warn!(error = %e, "failed to query jobs for gauge");
            return;
        }
    };

    let mut interval_count: usize = 0;
    let mut cron_count: usize = 0;

    for job in &all_jobs {
        if job.schedule.trim().starts_with("every:") {
            interval_count += 1;
        } else {
            cron_count += 1;
        }
    }

    metrics::gauge!(
        "sober_scheduler_jobs_registered",
        "type" => "interval",
        "persistence" => "persistent",
    )
    .set(interval_count as f64);
    metrics::gauge!(
        "sober_scheduler_jobs_registered",
        "type" => "cron",
        "persistence" => "persistent",
    )
    .set(cron_count as f64);
}
