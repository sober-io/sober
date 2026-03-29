//! Predefined system jobs registered idempotently on agent startup.

use sober_core::config::EvolutionConfig;
use sober_core::types::job_payload::{InternalOp, JobPayload};
use tracing::{info, warn};

use crate::SharedSchedulerClient;
use crate::grpc::scheduler_proto;

/// Well-known prompt text that identifies the self-evolution check job.
///
/// When the agent receives a Prompt job with this text, it intercepts it
/// and runs the custom 4-phase self-evolution cycle instead of the normal
/// LLM prompt pipeline.
pub const SELF_EVOLUTION_CHECK_PROMPT: &str = "self_evolution_check";

/// Definition of a system job to register on startup.
struct SystemJobDef {
    name: &'static str,
    schedule: String,
    payload: JobPayload,
}

/// Builds the list of predefined system jobs.
///
/// The evolution check interval is sourced from [`EvolutionConfig`] so it
/// can be overridden via environment variables or config file.
fn build_system_jobs(evolution_config: &EvolutionConfig) -> Vec<SystemJobDef> {
    // Format the evolution interval as a scheduler schedule string.
    // EvolutionConfig::interval stores e.g. "2h", "30m"; the scheduler
    // expects "every: 2h" for interval-based schedules.
    let evolution_schedule = format!("every: {}", evolution_config.interval);

    vec![
        SystemJobDef {
            name: "memory_pruning",
            schedule: "every: 1h".into(),
            payload: JobPayload::Internal {
                operation: InternalOp::MemoryPruning,
            },
        },
        SystemJobDef {
            name: "session_cleanup",
            schedule: "every: 6h".into(),
            payload: JobPayload::Internal {
                operation: InternalOp::SessionCleanup,
            },
        },
        SystemJobDef {
            name: "self_evolution_check",
            schedule: evolution_schedule,
            payload: JobPayload::Prompt {
                text: SELF_EVOLUTION_CHECK_PROMPT.into(),
                workspace_id: None,
                model_hint: None,
            },
        },
        SystemJobDef {
            name: "plugin_audit",
            // 7-field cron: sec min hour dom month dow year (dow: 1=Sun..7=Sat)
            schedule: "0 0 4 * * 2 *".into(),
            payload: JobPayload::Internal {
                operation: InternalOp::PluginAudit,
            },
        },
        SystemJobDef {
            name: "vector_index_optimize",
            schedule: "0 0 2 * * 1 *".into(),
            payload: JobPayload::Internal {
                operation: InternalOp::VectorIndexOptimize,
            },
        },
    ]
}

/// Registers predefined system jobs idempotently.
///
/// Skips any job that already exists by name. Safe to call on every startup.
pub async fn register_system_jobs(
    scheduler_client: &SharedSchedulerClient,
    evolution_config: &EvolutionConfig,
) {
    let mut client_guard = scheduler_client.write().await;
    let Some(client) = client_guard.as_mut() else {
        warn!("Scheduler not connected — skipping system job registration");
        return;
    };

    for def in build_system_jobs(evolution_config) {
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

        let payload_bytes = match serde_json::to_vec(&def.payload) {
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
                schedule: def.schedule.clone(),
                payload: payload_bytes,
                workspace_id: String::new(),
                created_by: String::new(),
                conversation_id: String::new(),
            })
            .await;

        match result {
            Ok(_) => info!(
                name = def.name,
                schedule = %def.schedule,
                "registered system job"
            ),
            Err(e) => warn!(name = def.name, error = %e, "failed to register system job"),
        }
    }
}
