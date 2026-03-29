//! Evolution execution logic.
//!
//! Takes an approved evolution event and executes it based on type:
//! - **Instruction** — fully implemented: writes overlay files, reloads mind
//! - **Plugin/Skill/Automation** — stub implementations pending infrastructure

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use sober_core::types::AgentRepos;
use sober_core::types::domain::EvolutionEvent;
use sober_core::types::enums::{EvolutionStatus, EvolutionType};
use sober_core::types::repo::EvolutionRepo;
use sober_mind::assembly::Mind;
use tracing::{error, info, warn};

use crate::error::AgentError;

/// Executes an approved evolution event.
///
/// 1. Atomically transitions the event from `Approved` to `Executing`.
/// 2. Dispatches to type-specific execution logic.
/// 3. On success: stores result and transitions to `Active`.
/// 4. On failure: stores error details and transitions to `Failed`.
///
/// If the event is not in `Approved` status (already picked up by another
/// trigger), this returns `Ok(())` silently.
pub async fn execute_evolution<R: AgentRepos>(
    event: &EvolutionEvent,
    repos: &R,
    mind: &Arc<Mind>,
) -> Result<(), AgentError> {
    // Atomic status guard: only proceed if currently approved.
    if event.status != EvolutionStatus::Approved {
        info!(
            event_id = %event.id,
            status = ?event.status,
            "evolution event not in approved status, skipping"
        );
        return Ok(());
    }

    // Transition to Executing.
    repos
        .evolution()
        .update_status(event.id, EvolutionStatus::Executing, None)
        .await
        .map_err(|e| AgentError::Internal(format!("failed to set executing status: {e}")))?;

    // Dispatch to type-specific execution.
    let result = match event.evolution_type {
        EvolutionType::Plugin => execute_plugin(event).await,
        EvolutionType::Skill => execute_skill(event).await,
        EvolutionType::Instruction => execute_instruction(event, mind).await,
        EvolutionType::Automation => execute_automation(event).await,
    };

    match result {
        Ok(result_value) => {
            repos
                .evolution()
                .update_result(event.id, result_value)
                .await
                .map_err(|e| AgentError::Internal(format!("failed to store result: {e}")))?;

            repos
                .evolution()
                .update_status(event.id, EvolutionStatus::Active, None)
                .await
                .map_err(|e| AgentError::Internal(format!("failed to set active status: {e}")))?;

            info!(event_id = %event.id, r#type = ?event.evolution_type, "evolution executed successfully");
            Ok(())
        }
        Err(e) => {
            let error_value = json!({ "error": e.to_string() });
            if let Err(store_err) = repos.evolution().update_result(event.id, error_value).await {
                error!(event_id = %event.id, error = %store_err, "failed to store error result");
            }
            if let Err(status_err) = repos
                .evolution()
                .update_status(event.id, EvolutionStatus::Failed, None)
                .await
            {
                error!(event_id = %event.id, error = %status_err, "failed to set failed status");
            }

            warn!(event_id = %event.id, error = %e, "evolution execution failed");
            Err(e)
        }
    }
}

/// Stub: plugin generation via `sober-plugin-gen`.
async fn execute_plugin(event: &EvolutionEvent) -> Result<Value, AgentError> {
    // TODO: Wire sober-plugin-gen pipeline:
    // 1. Extract plugin manifest from event.payload
    // 2. Generate WASM binary via PluginGenerator
    // 3. Install via PluginRepo
    // 4. Return plugin_id
    info!(
        event_id = %event.id,
        title = %event.title,
        "plugin evolution execution (stub — pending sober-plugin-gen integration)"
    );
    Ok(json!({ "plugin_id": "pending-implementation" }))
}

/// Stub: skill creation as a prompt-based plugin.
async fn execute_skill(event: &EvolutionEvent) -> Result<Value, AgentError> {
    // TODO: Wire skill creation pipeline:
    // 1. Extract skill content from event.payload
    // 2. Create plugin entry with PluginKind::Skill
    // 3. Write skill file to workspace
    // 4. Reload skill catalog
    // 5. Return plugin_id + skill_path
    info!(
        event_id = %event.id,
        title = %event.title,
        "skill evolution execution (stub — pending skill creation pipeline)"
    );
    Ok(json!({ "plugin_id": "pending-implementation", "skill_path": "pending" }))
}

/// Fully implemented: writes instruction overlay file and reloads the mind.
async fn execute_instruction(
    event: &EvolutionEvent,
    mind: &Arc<Mind>,
) -> Result<Value, AgentError> {
    let file = event
        .payload
        .get("file")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AgentError::Internal("instruction payload missing 'file' field".into()))?;

    let new_content = event
        .payload
        .get("new_content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AgentError::Internal("instruction payload missing 'new_content' field".into())
        })?;

    // Validate that this is not a guardrail file.
    if sober_mind::is_guardrail_file(file, new_content) {
        return Err(AgentError::Internal(format!(
            "cannot modify guardrail instruction file: {file}"
        )));
    }

    // Resolve the overlay directory: ~/.sober/instructions/
    let overlay_dir = resolve_overlay_dir()?;

    // Ensure the directory exists.
    std::fs::create_dir_all(&overlay_dir).map_err(|e| {
        AgentError::Internal(format!(
            "failed to create overlay directory {}: {e}",
            overlay_dir.display()
        ))
    })?;

    // Write the overlay file.
    let overlay_path = overlay_dir.join(file);
    std::fs::write(&overlay_path, new_content).map_err(|e| {
        AgentError::Internal(format!(
            "failed to write overlay file {}: {e}",
            overlay_path.display()
        ))
    })?;

    info!(file = %file, path = %overlay_path.display(), "instruction overlay written");

    // Reload instructions so the change takes effect immediately.
    mind.reload_instructions().map_err(|e| {
        AgentError::Internal(format!("failed to reload instructions after write: {e}"))
    })?;

    Ok(json!({}))
}

/// Stub: scheduled job creation via scheduler gRPC.
async fn execute_automation(event: &EvolutionEvent) -> Result<Value, AgentError> {
    // TODO: Wire scheduler job creation:
    // 1. Extract schedule spec from event.payload
    // 2. Create job via SchedulerServiceClient
    // 3. Return job_id
    info!(
        event_id = %event.id,
        title = %event.title,
        "automation evolution execution (stub — pending scheduler integration)"
    );
    Ok(json!({ "job_id": "pending-implementation" }))
}

/// Resolves the instruction overlay directory path (`~/.sober/instructions/`).
pub(crate) fn resolve_overlay_dir() -> Result<PathBuf, AgentError> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| AgentError::Internal("HOME environment variable not set".into()))?;
    Ok(PathBuf::from(home).join(".sober").join("instructions"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_overlay_dir_returns_expected_path() {
        // This test relies on HOME being set (standard in CI and dev).
        if let Ok(dir) = resolve_overlay_dir() {
            assert!(dir.to_string_lossy().ends_with(".sober/instructions"));
        }
    }
}
