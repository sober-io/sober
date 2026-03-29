//! Evolution revert logic.
//!
//! Reverts an active evolution event back to its previous state:
//! - **Instruction** — fully implemented: restores previous overlay or deletes it
//! - **Plugin/Skill/Automation** — stub implementations pending infrastructure

use std::sync::Arc;

use sober_core::types::AgentRepos;
use sober_core::types::domain::EvolutionEvent;
use sober_core::types::enums::{EvolutionStatus, EvolutionType};
use sober_core::types::repo::EvolutionRepo;
use sober_mind::assembly::Mind;
use tracing::{error, info, warn};

use crate::error::AgentError;

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
) -> Result<(), AgentError> {
    if event.status != EvolutionStatus::Active {
        return Err(AgentError::Internal(format!(
            "cannot revert evolution event {} — status is {:?}, expected Active",
            event.id, event.status
        )));
    }

    let result = match event.evolution_type {
        EvolutionType::Plugin => revert_plugin(event).await,
        EvolutionType::Skill => revert_skill(event).await,
        EvolutionType::Instruction => revert_instruction(event, mind).await,
        EvolutionType::Automation => revert_automation(event).await,
    };

    match result {
        Ok(()) => {
            repos
                .evolution()
                .update_status(event.id, EvolutionStatus::Reverted, None)
                .await
                .map_err(|e| AgentError::Internal(format!("failed to set reverted status: {e}")))?;

            info!(event_id = %event.id, r#type = ?event.evolution_type, "evolution reverted successfully");
            Ok(())
        }
        Err(e) => {
            error!(
                event_id = %event.id,
                error = %e,
                "evolution revert failed — status unchanged"
            );
            Err(e)
        }
    }
}

/// Stub: plugin removal.
async fn revert_plugin(event: &EvolutionEvent) -> Result<(), AgentError> {
    // TODO: Wire PluginRepo::delete to remove the installed plugin.
    info!(
        event_id = %event.id,
        title = %event.title,
        "plugin evolution revert (stub — pending PluginRepo::delete integration)"
    );
    Ok(())
}

/// Stub: skill removal.
async fn revert_skill(event: &EvolutionEvent) -> Result<(), AgentError> {
    // TODO: Wire plugin delete + skill file removal + skill catalog reload.
    info!(
        event_id = %event.id,
        title = %event.title,
        "skill evolution revert (stub — pending skill removal pipeline)"
    );
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

/// Stub: automation job cancellation.
async fn revert_automation(event: &EvolutionEvent) -> Result<(), AgentError> {
    // TODO: Wire JobRepo::cancel to cancel the scheduled job.
    info!(
        event_id = %event.id,
        title = %event.title,
        "automation evolution revert (stub — pending JobRepo::cancel integration)"
    );
    Ok(())
}
