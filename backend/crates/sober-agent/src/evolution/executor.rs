//! Evolution execution logic.
//!
//! Takes an approved evolution event and executes it based on type:
//! - **Instruction** — writes overlay files, reloads mind
//! - **Plugin** — registers plugin via audit pipeline (WASM generation requires LLM)
//! - **Skill** — writes skill file to disk, registers plugin, invalidates skill cache
//! - **Automation** — creates scheduled job via scheduler gRPC

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use sober_core::types::AgentRepos;
use sober_core::types::JobPayload;
use sober_core::types::domain::EvolutionEvent;
use sober_core::types::enums::{
    EvolutionStatus, EvolutionType, PluginKind, PluginOrigin, PluginScope,
};
use sober_core::types::repo::{EvolutionRepo, PluginRepo};
use sober_mind::assembly::Mind;
use sober_plugin::PluginManager;
use sober_plugin::registry::InstallRequest;
use tracing::{error, info, warn};

use crate::SharedSchedulerClient;
use crate::error::AgentError;
use crate::grpc::scheduler_proto;

/// Extra dependencies needed by evolution executors beyond `repos` and `mind`.
///
/// Bundles infrastructure that is only available at the agent level (scheduler
/// gRPC client, plugin manager with audit pipeline and skill loader). Callers
/// construct this from the `Agent` / `AgentGrpcService` fields.
pub struct EvolutionContext<R: AgentRepos> {
    /// Shared gRPC client for the scheduler service.
    pub scheduler_client: SharedSchedulerClient,
    /// Plugin manager for audit pipeline + skill loader access.
    pub plugin_manager: Arc<PluginManager<R::Plg>>,
}

/// Converts an [`EvolutionType`] to a static string for metric labels.
fn evolution_type_str(t: EvolutionType) -> &'static str {
    match t {
        EvolutionType::Plugin => "plugin",
        EvolutionType::Skill => "skill",
        EvolutionType::Instruction => "instruction",
        EvolutionType::Automation => "automation",
    }
}

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
    ctx: &EvolutionContext<R>,
) -> Result<(), AgentError> {
    info!(
        event_id = %event.id,
        evolution_type = ?event.evolution_type,
        title = %event.title,
        "executing evolution"
    );

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
    let type_str = evolution_type_str(event.evolution_type);
    let exec_start = std::time::Instant::now();

    let result = match event.evolution_type {
        EvolutionType::Plugin => execute_plugin(event, &*ctx.plugin_manager).await,
        EvolutionType::Skill => execute_skill(event, &*ctx.plugin_manager).await,
        EvolutionType::Instruction => execute_instruction(event, mind).await,
        EvolutionType::Automation => execute_automation(event, &ctx.scheduler_client).await,
    };

    metrics::histogram!("sober_evolution_execution_duration_seconds", "type" => type_str)
        .record(exec_start.elapsed().as_secs_f64());

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

            metrics::counter!("sober_evolution_events_total", "type" => type_str, "status" => "active")
                .increment(1);

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

            metrics::counter!("sober_evolution_events_total", "type" => type_str, "status" => "failed")
                .increment(1);

            warn!(event_id = %event.id, evolution_type = ?event.evolution_type, error = %e, "evolution execution failed");
            Err(e)
        }
    }
}

/// Registers a plugin via the audit pipeline.
///
/// Extracts `name`, `description`, and `capabilities` from the event payload
/// and installs through `PluginRegistry::install`. WASM generation (which
/// requires an LLM client) is not performed here — the plugin is registered
/// as a manifest-only entry that can be compiled later via `GeneratePluginTool`.
///
/// TODO: Wire `PluginGenerator::generate_wasm` for full WASM compilation.
/// This requires threading an `Arc<PluginGenerator>` (which holds an LLM
/// client) into `EvolutionContext`. Currently the generator lives on
/// `ToolBootstrap` and is `Option<Arc<PluginGenerator>>`.
async fn execute_plugin<P: PluginRepo>(
    event: &EvolutionEvent,
    plugin_manager: &PluginManager<P>,
) -> Result<Value, AgentError> {
    let name = event
        .payload
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AgentError::Internal("plugin payload missing 'name' field".into()))?;

    let description = event
        .payload
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let capabilities: Vec<String> = event
        .payload
        .get("capabilities")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let user_id = event.user_id;

    info!(
        event_id = %event.id,
        plugin_name = %name,
        capabilities = ?capabilities,
        "executing plugin evolution"
    );

    let config = json!({
        "capabilities": capabilities,
        "pseudocode": event.payload.get("pseudocode").cloned().unwrap_or(Value::Null),
    });

    let install_req = InstallRequest {
        name: name.to_owned(),
        kind: PluginKind::Wasm,
        version: Some("0.1.0".to_owned()),
        description: Some(description.to_owned()),
        origin: PluginOrigin::Agent,
        scope: PluginScope::User,
        owner_id: user_id,
        workspace_id: None,
        config,
        installed_by: user_id,
        manifest: None,
        wasm_bytes: None,
    };

    let report = plugin_manager
        .registry()
        .install(install_req)
        .await
        .map_err(|e| AgentError::Internal(format!("failed to install plugin: {e}")))?;

    if !report.is_approved() {
        return Err(AgentError::Internal(format!(
            "plugin rejected by audit: {}",
            report.rejection_reason().unwrap_or("unknown")
        )));
    }

    // Look up the just-registered plugin to get its ID.
    let plugin = plugin_manager
        .registry()
        .repo()
        .get_by_name(name)
        .await
        .map_err(|e| AgentError::Internal(format!("failed to look up registered plugin: {e}")))?;

    info!(
        event_id = %event.id,
        plugin_id = %plugin.id,
        "plugin registered via audit pipeline"
    );

    Ok(json!({ "plugin_id": plugin.id.to_string() }))
}

/// Creates a skill file on disk and registers it as a plugin.
///
/// Writes a `SKILL.md` with YAML frontmatter to the user-level skill directory
/// (`~/.sober/skills/<name>/SKILL.md`) so that [`SkillLoader`] discovers it on
/// the next scan. The skill loader cache is invalidated to force immediate
/// rediscovery.
async fn execute_skill<P: PluginRepo>(
    event: &EvolutionEvent,
    plugin_manager: &PluginManager<P>,
) -> Result<Value, AgentError> {
    let name = event
        .payload
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AgentError::Internal("skill payload missing 'name' field".into()))?;

    let description = event
        .payload
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let prompt_template = event
        .payload
        .get("prompt_template")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AgentError::Internal("skill payload missing 'prompt_template' field".into())
        })?;

    let user_id = event.user_id;

    info!(
        event_id = %event.id,
        skill_name = %name,
        "executing skill evolution"
    );

    // Build the SKILL.md content with YAML frontmatter.
    let skill_content =
        format!("---\nname: {name}\ndescription: {description}\n---\n\n{prompt_template}");

    // Write to user-level skill directory: ~/.sober/skills/<name>/SKILL.md
    let home = std::env::var_os("HOME")
        .ok_or_else(|| AgentError::Internal("HOME environment variable not set".into()))?;
    let skill_dir = PathBuf::from(home).join(".sober").join("skills").join(name);
    tokio::fs::create_dir_all(&skill_dir).await.map_err(|e| {
        AgentError::Internal(format!(
            "failed to create skill directory {}: {e}",
            skill_dir.display()
        ))
    })?;

    let skill_path = skill_dir.join("SKILL.md");
    tokio::fs::write(&skill_path, &skill_content)
        .await
        .map_err(|e| {
            AgentError::Internal(format!(
                "failed to write skill file {}: {e}",
                skill_path.display()
            ))
        })?;

    info!(
        skill_name = %name,
        path = %skill_path.display(),
        "skill file written"
    );

    // Register in the plugin system through the audit pipeline.
    let config = json!({
        "path": skill_path.to_string_lossy(),
    });

    let install_req = InstallRequest {
        name: name.to_owned(),
        kind: PluginKind::Skill,
        version: Some("0.1.0".to_owned()),
        description: Some(description.to_owned()),
        origin: PluginOrigin::Agent,
        scope: PluginScope::User,
        owner_id: user_id,
        workspace_id: None,
        config,
        installed_by: user_id,
        manifest: None,
        wasm_bytes: None,
    };

    let report = plugin_manager
        .registry()
        .install(install_req)
        .await
        .map_err(|e| AgentError::Internal(format!("failed to register skill plugin: {e}")))?;

    if !report.is_approved() {
        // Clean up the written file on rejection.
        let _ = tokio::fs::remove_dir_all(&skill_dir).await;
        return Err(AgentError::Internal(format!(
            "skill rejected by audit: {}",
            report.rejection_reason().unwrap_or("unknown")
        )));
    }

    // Look up the just-registered plugin to get its ID.
    let plugin = plugin_manager
        .registry()
        .repo()
        .get_by_name(name)
        .await
        .map_err(|e| AgentError::Internal(format!("failed to look up registered skill: {e}")))?;

    // Invalidate the skill loader cache so the new skill is discovered immediately.
    plugin_manager.skill_loader().invalidate_cache();

    info!(
        event_id = %event.id,
        plugin_id = %plugin.id,
        path = %skill_path.display(),
        "skill registered and cache invalidated"
    );

    Ok(json!({
        "plugin_id": plugin.id.to_string(),
        "skill_path": skill_path.to_string_lossy(),
    }))
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

/// Creates a scheduled job via the scheduler gRPC service.
///
/// Extracts `job_name`, `schedule`, `prompt`, `target_user_id`, and optional
/// `conversation_id` from the event payload. The job payload is
/// [`JobPayload::Prompt`] so the scheduler dispatches it to the agent for
/// LLM processing.
async fn execute_automation(
    event: &EvolutionEvent,
    scheduler_client: &SharedSchedulerClient,
) -> Result<Value, AgentError> {
    let job_name = event
        .payload
        .get("job_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AgentError::Internal("automation payload missing 'job_name' field".into())
        })?;

    let schedule = event
        .payload
        .get("schedule")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AgentError::Internal("automation payload missing 'schedule' field".into())
        })?;

    let prompt = event
        .payload
        .get("prompt")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AgentError::Internal("automation payload missing 'prompt' field".into()))?;

    let target_user_id_str = event
        .payload
        .get("target_user_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AgentError::Internal("automation payload missing 'target_user_id' field".into())
        })?;

    let target_user_id = uuid::Uuid::parse_str(target_user_id_str)
        .map_err(|e| AgentError::Internal(format!("invalid target_user_id UUID: {e}")))?;

    let conversation_id = event
        .payload
        .get("conversation_id")
        .and_then(|v| v.as_str())
        .and_then(|s| uuid::Uuid::parse_str(s).ok());

    info!(
        event_id = %event.id,
        job_name = %job_name,
        schedule = %schedule,
        "executing automation evolution"
    );

    // Build the Prompt job payload.
    let job_payload = JobPayload::Prompt {
        text: prompt.to_owned(),
        workspace_id: None,
        model_hint: None,
    };
    let payload_bytes = serde_json::to_vec(&job_payload)
        .map_err(|e| AgentError::Internal(format!("failed to serialize job payload: {e}")))?;

    // Create the job via scheduler gRPC.
    let req = scheduler_proto::CreateJobRequest {
        name: job_name.to_owned(),
        owner_type: "user".to_owned(),
        owner_id: Some(target_user_id.to_string()),
        schedule: schedule.to_owned(),
        payload: payload_bytes,
        workspace_id: String::new(),
        created_by: target_user_id.to_string(),
        conversation_id: conversation_id.map(|id| id.to_string()).unwrap_or_default(),
    };

    let mut client = {
        let guard = scheduler_client.read().await;
        guard
            .as_ref()
            .ok_or_else(|| AgentError::Internal("scheduler not connected".into()))?
            .clone()
    };

    let response = client
        .create_job(req)
        .await
        .map_err(|e| AgentError::Internal(format!("scheduler CreateJob failed: {e}")))?;

    let job = response.into_inner();

    info!(
        event_id = %event.id,
        job_id = %job.id,
        job_name = %job.name,
        next_run = %job.next_run_at,
        "automation job created"
    );

    Ok(json!({ "job_id": job.id }))
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
