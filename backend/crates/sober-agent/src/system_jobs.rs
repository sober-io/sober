//! Predefined system jobs registered idempotently on agent startup.

use sober_core::types::job_payload::{InternalOp, JobPayload};
use tracing::{info, warn};

use crate::SharedSchedulerClient;
use crate::grpc::scheduler_proto;

/// Definition of a system job to register on startup.
struct SystemJobDef {
    name: &'static str,
    schedule: &'static str,
    payload: JobPayload,
}

/// All predefined system jobs. Each closure produces a definition.
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

/// Registers predefined system jobs idempotently.
///
/// Skips any job that already exists by name. Safe to call on every startup.
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
                owner_type: Some("system".into()),
                name_filter: def.name.into(),
                ..Default::default()
            })
            .await;

        match existing {
            Ok(resp) => {
                if !resp.into_inner().jobs.is_empty() {
                    info!(name = def.name, "system job already registered, skipping");
                    continue;
                }
            }
            Err(e) => {
                warn!(name = def.name, error = %e, "failed to check system job, skipping");
                continue;
            }
        }

        let payload_bytes = match def.payload.to_bytes() {
            Ok(b) => b,
            Err(e) => {
                warn!(name = def.name, error = %e, "failed to serialize system job payload");
                continue;
            }
        };

        let result = client
            .create_job(scheduler_proto::CreateJobRequest {
                name: def.name.into(),
                owner_type: "system".into(),
                owner_id: None,
                schedule: def.schedule.into(),
                payload: payload_bytes,
                workspace_id: String::new(),
                created_by: String::new(),
                conversation_id: String::new(),
            })
            .await;

        match result {
            Ok(_) => info!(
                name = def.name,
                schedule = def.schedule,
                "registered system job"
            ),
            Err(e) => warn!(name = def.name, error = %e, "failed to register system job"),
        }
    }
}
