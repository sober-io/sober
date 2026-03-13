//! System job definitions and idempotent registration.
//!
//! System jobs are deterministic maintenance tasks (memory pruning, session
//! cleanup) that the scheduler executes locally. They are registered once at
//! startup and skip creation if a job with the same name already exists.

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
    op: &'static str,
}

/// All built-in system jobs.
const SYSTEM_JOBS: &[SystemJobDef] = &[
    SystemJobDef {
        name: "system::memory_pruning",
        schedule: "every: 1h",
        op: "memory_pruning",
    },
    SystemJobDef {
        name: "system::session_cleanup",
        schedule: "every: 15m",
        op: "session_cleanup",
    },
];

/// Register all built-in system jobs, skipping any that already exist.
///
/// Called once at scheduler startup. Jobs are identified by name — if a job
/// with the same name already exists (any status), registration is skipped.
pub async fn register_system_jobs<J: JobRepo>(job_repo: &J) -> Result<(), AppError> {
    // Fetch existing system jobs to check for duplicates.
    let existing = job_repo
        .list_filtered(Some("system"), None, None, None, None)
        .await?;

    for def in SYSTEM_JOBS {
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

        let payload = serde_json::json!({
            "type": "internal",
            "op": def.op,
        });

        let input = CreateJob {
            name: def.name.to_owned(),
            schedule: def.schedule.to_owned(),
            payload,
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

    Ok(())
}
