//! Evolution revert logic.
//!
//! Reverts an active evolution event back to its previous state:
//! - **Instruction** — restores previous overlay or deletes it, reloads mind
//! - **Plugin** — deletes plugin from registry
//! - **Skill** — deletes plugin, removes skill file, invalidates skill cache
//! - **Automation** — cancels scheduled job via scheduler gRPC

use std::sync::Arc;

use sober_core::types::AgentRepos;
use sober_core::types::domain::EvolutionEvent;
use sober_core::types::enums::{EvolutionStatus, EvolutionType};
use sober_core::types::ids::PluginId;
use sober_core::types::repo::{EvolutionRepo, PluginRepo};
use sober_mind::assembly::Mind;
use sober_plugin::PluginManager;
use tracing::{info, warn};

use super::executor::EvolutionContext;
use crate::SharedSchedulerClient;
use crate::error::AgentError;
use crate::grpc::scheduler_proto;

/// Converts an [`EvolutionType`] to a static string for metric labels.
fn evolution_type_str(t: EvolutionType) -> &'static str {
    match t {
        EvolutionType::Plugin => "plugin",
        EvolutionType::Skill => "skill",
        EvolutionType::Instruction => "instruction",
        EvolutionType::Automation => "automation",
    }
}

/// Reverts an active evolution event.
///
/// 1. Validates the event is in `Active` status.
/// 2. Dispatches to type-specific revert logic.
/// 3. On success: transitions to `Reverted`.
/// 4. On failure: logs error but does not change status.
pub async fn revert_evolution<R: AgentRepos>(
    event: &EvolutionEvent,
    repos: &R,
    mind: &Arc<Mind>,
    ctx: &EvolutionContext<R>,
) -> Result<(), AgentError> {
    info!(
        event_id = %event.id,
        evolution_type = ?event.evolution_type,
        title = %event.title,
        "reverting evolution"
    );

    if event.status != EvolutionStatus::Active {
        return Err(AgentError::Internal(format!(
            "cannot revert evolution event {} — status is {:?}, expected Active",
            event.id, event.status
        )));
    }

    let result = match event.evolution_type {
        EvolutionType::Plugin => revert_plugin(event, &*ctx.plugin_manager).await,
        EvolutionType::Skill => revert_skill(event, &*ctx.plugin_manager).await,
        EvolutionType::Instruction => revert_instruction(event, mind).await,
        EvolutionType::Automation => revert_automation(event, &ctx.scheduler_client).await,
    };

    match result {
        Ok(()) => {
            repos
                .evolution()
                .update_status(event.id, EvolutionStatus::Reverted, None)
                .await
                .map_err(|e| AgentError::Internal(format!("failed to set reverted status: {e}")))?;

            metrics::counter!("sober_evolution_reverts_total", "type" => evolution_type_str(event.evolution_type))
                .increment(1);

            info!(event_id = %event.id, r#type = ?event.evolution_type, "evolution reverted successfully");
            Ok(())
        }
        Err(e) => {
            warn!(
                event_id = %event.id,
                evolution_type = ?event.evolution_type,
                error = %e,
                "evolution revert failed — status unchanged"
            );
            Err(e)
        }
    }
}

/// Deletes a plugin from the registry.
///
/// Extracts `plugin_id` from the event's execution result (stored when the
/// plugin was created). Falls back to looking up by name from the payload.
async fn revert_plugin<P: PluginRepo>(
    event: &EvolutionEvent,
    plugin_manager: &PluginManager<P>,
) -> Result<(), AgentError> {
    let plugin_id = resolve_plugin_id(event, plugin_manager.registry().repo()).await?;

    info!(
        event_id = %event.id,
        plugin_id = %plugin_id,
        "deleting plugin"
    );

    plugin_manager
        .registry()
        .repo()
        .delete(plugin_id)
        .await
        .map_err(|e| AgentError::Internal(format!("failed to delete plugin: {e}")))?;

    info!(event_id = %event.id, plugin_id = %plugin_id, "plugin deleted");
    Ok(())
}

/// Deletes a skill's plugin entry, removes the skill file from disk, and
/// invalidates the skill loader cache.
async fn revert_skill<P: PluginRepo>(
    event: &EvolutionEvent,
    plugin_manager: &PluginManager<P>,
) -> Result<(), AgentError> {
    let plugin_id = resolve_plugin_id(event, plugin_manager.registry().repo()).await?;

    info!(
        event_id = %event.id,
        plugin_id = %plugin_id,
        "deleting skill plugin"
    );

    // Delete plugin from registry.
    plugin_manager
        .registry()
        .repo()
        .delete(plugin_id)
        .await
        .map_err(|e| AgentError::Internal(format!("failed to delete skill plugin: {e}")))?;

    // Remove skill file from disk (best-effort — the plugin deletion is the
    // important part; the file will just be ignored by the loader if orphaned).
    let skill_path = event
        .result
        .as_ref()
        .and_then(|r| r.get("skill_path"))
        .and_then(|v| v.as_str());

    if let Some(path) = skill_path {
        // The skill directory is the parent of SKILL.md.
        let skill_dir = std::path::Path::new(path).parent();
        if let Some(dir) = skill_dir {
            match tokio::fs::remove_dir_all(dir).await {
                Ok(()) => {
                    info!(path = %dir.display(), "skill directory removed");
                }
                Err(e) => {
                    warn!(
                        path = %dir.display(),
                        error = %e,
                        "failed to remove skill directory (non-fatal)"
                    );
                }
            }
        }
    }

    // Invalidate the skill loader cache so the removed skill is no longer discovered.
    plugin_manager.skill_loader().invalidate_cache();

    info!(event_id = %event.id, "skill reverted");
    Ok(())
}

/// Fully implemented: restores previous instruction overlay or removes the overlay file.
async fn revert_instruction(event: &EvolutionEvent, mind: &Arc<Mind>) -> Result<(), AgentError> {
    let file = event
        .payload
        .get("file")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AgentError::Internal("instruction payload missing 'file' field".into()))?;

    let overlay_dir = super::executor::resolve_overlay_dir()?;
    let overlay_path = overlay_dir.join(file);

    let previous_content = event
        .payload
        .get("previous_content")
        .and_then(|v| v.as_str());

    match previous_content {
        Some(content) => {
            // Restore previous overlay content.
            std::fs::write(&overlay_path, content).map_err(|e| {
                AgentError::Internal(format!(
                    "failed to restore overlay file {}: {e}",
                    overlay_path.display()
                ))
            })?;
            info!(file = %file, "instruction overlay restored to previous content");
        }
        None => {
            // No previous content — delete the overlay file so the base instruction
            // takes effect again.
            if overlay_path.exists() {
                std::fs::remove_file(&overlay_path).map_err(|e| {
                    AgentError::Internal(format!(
                        "failed to remove overlay file {}: {e}",
                        overlay_path.display()
                    ))
                })?;
                info!(file = %file, "instruction overlay removed (reverting to base)");
            } else {
                warn!(file = %file, "overlay file already absent during revert");
            }
        }
    }

    // Reload instructions so the revert takes effect immediately.
    mind.reload_instructions().map_err(|e| {
        AgentError::Internal(format!("failed to reload instructions after revert: {e}"))
    })?;

    Ok(())
}

/// Cancels a scheduled job via the scheduler gRPC service.
///
/// Extracts `job_id` from the event's execution result.
async fn revert_automation(
    event: &EvolutionEvent,
    scheduler_client: &SharedSchedulerClient,
) -> Result<(), AgentError> {
    let job_id = event
        .result
        .as_ref()
        .and_then(|r| r.get("job_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AgentError::Internal("automation result missing 'job_id' — cannot cancel".into())
        })?;

    info!(
        event_id = %event.id,
        job_id = %job_id,
        "cancelling automation job"
    );

    let req = scheduler_proto::CancelJobRequest {
        job_id: job_id.to_owned(),
    };

    let mut client = {
        let guard = scheduler_client.read().await;
        guard
            .as_ref()
            .ok_or_else(|| AgentError::Internal("scheduler not connected".into()))?
            .clone()
    };

    client
        .cancel_job(req)
        .await
        .map_err(|e| AgentError::Internal(format!("scheduler CancelJob failed: {e}")))?;

    info!(event_id = %event.id, job_id = %job_id, "automation job cancelled");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolves the `PluginId` for a plugin/skill revert.
///
/// First tries to extract from the event's execution result (`plugin_id`
/// field). Falls back to looking up by name from the payload.
async fn resolve_plugin_id<P: PluginRepo>(
    event: &EvolutionEvent,
    repo: &P,
) -> Result<PluginId, AgentError> {
    // Try result first — stored when the evolution was executed.
    if let Some(id_str) = event
        .result
        .as_ref()
        .and_then(|r| r.get("plugin_id"))
        .and_then(|v| v.as_str())
        && let Ok(uuid) = uuid::Uuid::parse_str(id_str)
    {
        return Ok(PluginId::from_uuid(uuid));
    }

    // Fallback: look up by name from the payload.
    let name = event
        .payload
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AgentError::Internal(
                "cannot resolve plugin: no plugin_id in result and no name in payload".into(),
            )
        })?;

    let plugin = repo
        .get_by_name(name)
        .await
        .map_err(|e| AgentError::Internal(format!("plugin lookup by name '{name}' failed: {e}")))?;

    Ok(plugin.id)
}
