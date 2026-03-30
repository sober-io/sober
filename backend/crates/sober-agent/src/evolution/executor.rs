//! Evolution execution logic.
//!
//! Takes an approved evolution event and executes it based on type:
//! - **Plugin** — generates WASM via [`PluginGenerator`], registers via audit pipeline
//! - **Skill** — generates skill content via [`PluginGenerator`], writes to disk, registers
//! - **Instruction** — writes overlay files, reloads mind
//! - **Automation** — creates scheduled job via scheduler gRPC

use std::sync::Arc;

use serde_json::{Value, json};
use sober_core::config::EvolutionConfig;
use sober_core::types::AgentRepos;
use sober_core::types::JobPayload;
use sober_core::types::domain::EvolutionEvent;
use sober_core::types::enums::{
    EvolutionStatus, EvolutionType, PluginKind, PluginOrigin, PluginScope,
};
use sober_core::types::repo::{EvolutionRepo, PluginRepo};
use sober_mind::assembly::Mind;
use sober_plugin::PluginManager;
use sober_plugin::manifest::PluginManifest;
use sober_plugin::registry::InstallRequest;
use sober_plugin_gen::PluginGenerator;
use tracing::{error, info, warn};

use crate::SharedSchedulerClient;
use crate::error::AgentError;
use crate::grpc::scheduler_proto;

/// Extra dependencies needed by evolution executors beyond `repos` and `mind`.
///
/// Bundles infrastructure that is only available at the agent level (scheduler
/// gRPC client, plugin manager with audit pipeline and skill loader, and the
/// LLM-powered plugin generator). Callers construct this from the
/// `Agent` / `AgentGrpcService` fields.
pub struct EvolutionContext<R: AgentRepos> {
    /// Shared gRPC client for the scheduler service.
    pub scheduler_client: SharedSchedulerClient,
    /// Plugin manager for audit pipeline + skill loader access.
    pub plugin_manager: Arc<PluginManager<R::Plg>>,
    /// LLM-powered plugin/skill generator.
    pub plugin_generator: Arc<PluginGenerator>,
    /// Evolution configuration (detection limits, LLM params).
    pub evolution_config: EvolutionConfig,
}

/// Converts an [`EvolutionType`] to a static string for metric labels.
pub(crate) fn evolution_type_str(t: EvolutionType) -> &'static str {
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
    let type_str = evolution_type_str(event.evolution_type);

    info!(
        event_id = %event.id,
        evolution_type = type_str,
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

    let exec_start = std::time::Instant::now();

    let result = match event.evolution_type {
        EvolutionType::Plugin => execute_plugin(event, ctx).await,
        EvolutionType::Skill => execute_skill(event, ctx).await,
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

            info!(event_id = %event.id, evolution_type = type_str, "evolution executed successfully");
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

            warn!(event_id = %event.id, evolution_type = type_str, error = %e, "evolution execution failed");
            Err(e)
        }
    }
}

/// Generates a WASM plugin via [`PluginGenerator`] and registers it through
/// the audit pipeline.
///
/// Extracts `name`, `description`, and `capabilities` from the event payload.
/// The generator produces compiled WASM bytes + a manifest, which are passed
/// directly to `PluginRegistry::install`.
async fn execute_plugin<R: AgentRepos>(
    event: &EvolutionEvent,
    ctx: &EvolutionContext<R>,
) -> Result<Value, AgentError> {
    let name = payload_str(event, "name")?;
    let description = event
        .payload
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let capabilities = payload_string_array(event, "capabilities");

    info!(
        event_id = %event.id,
        plugin_name = %name,
        capabilities = ?capabilities,
        "generating WASM plugin"
    );

    // Generate WASM via LLM with self-correcting retry loop.
    let generated = ctx
        .plugin_generator
        .generate_wasm(name, description, &capabilities)
        .await
        .map_err(|e| AgentError::Internal(format!("WASM generation failed: {e}")))?;

    // Parse the manifest from the generated TOML.
    let manifest = PluginManifest::from_toml(&generated.manifest)
        .map_err(|e| AgentError::Internal(format!("invalid generated manifest: {e}")))?;

    // Install through the audit pipeline with actual WASM bytes.
    let install_req = InstallRequest {
        name: name.to_owned(),
        kind: PluginKind::Wasm,
        version: Some("0.1.0".to_owned()),
        description: Some(description.to_owned()),
        origin: PluginOrigin::Agent,
        scope: PluginScope::User,
        owner_id: event.user_id,
        workspace_id: None,
        config: json!({
            "capabilities": capabilities,
            "source": generated.source,
        }),
        installed_by: event.user_id,
        manifest: Some(manifest),
        wasm_bytes: Some(generated.wasm_bytes),
        reserved_tool_names: vec![],
    };

    let report = ctx
        .plugin_manager
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

    let plugin = ctx
        .plugin_manager
        .registry()
        .repo()
        .get_by_name(name)
        .await
        .map_err(|e| AgentError::Internal(format!("failed to look up registered plugin: {e}")))?;

    info!(
        event_id = %event.id,
        plugin_id = %plugin.id,
        "plugin generated and registered via audit pipeline"
    );

    Ok(json!({ "plugin_id": plugin.id.to_string() }))
}

/// Generates a skill via [`PluginGenerator`], writes it to disk, and registers
/// it as a plugin.
///
/// The generator produces markdown content with YAML frontmatter. The file is
/// written to `~/.sober/skills/<name>/SKILL.md` so that [`SkillLoader`]
/// discovers it on the next scan. The skill loader cache is invalidated to
/// force immediate rediscovery.
async fn execute_skill<R: AgentRepos>(
    event: &EvolutionEvent,
    ctx: &EvolutionContext<R>,
) -> Result<Value, AgentError> {
    let name = payload_str(event, "name")?;
    let description = event
        .payload
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    info!(
        event_id = %event.id,
        skill_name = %name,
        "generating skill"
    );

    // Generate skill content via LLM.
    let skill_content = ctx
        .plugin_generator
        .generate_skill(name, description)
        .await
        .map_err(|e| AgentError::Internal(format!("skill generation failed: {e}")))?;

    // Write to user-level skill directory: ~/.sober/skills/<name>/SKILL.md
    let skill_dir = sober_skill::SkillLoader::resolve_skill_dir(name)
        .map_err(|e| AgentError::Internal(format!("failed to resolve skill directory: {e}")))?;
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
    let install_req = InstallRequest {
        name: name.to_owned(),
        kind: PluginKind::Skill,
        version: Some("0.1.0".to_owned()),
        description: Some(description.to_owned()),
        origin: PluginOrigin::Agent,
        scope: PluginScope::User,
        owner_id: event.user_id,
        workspace_id: None,
        config: json!({ "path": skill_path.to_string_lossy() }),
        installed_by: event.user_id,
        manifest: None,
        wasm_bytes: None,
        reserved_tool_names: vec![],
    };

    let report = ctx
        .plugin_manager
        .registry()
        .install(install_req)
        .await
        .map_err(|e| AgentError::Internal(format!("failed to register skill plugin: {e}")))?;

    if !report.is_approved() {
        let _ = tokio::fs::remove_dir_all(&skill_dir).await;
        return Err(AgentError::Internal(format!(
            "skill rejected by audit: {}",
            report.rejection_reason().unwrap_or("unknown")
        )));
    }

    let plugin = ctx
        .plugin_manager
        .registry()
        .repo()
        .get_by_name(name)
        .await
        .map_err(|e| AgentError::Internal(format!("failed to look up registered skill: {e}")))?;

    // Invalidate the skill loader cache so the new skill is discovered immediately.
    ctx.plugin_manager.skill_loader().invalidate_cache();

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

/// Writes an instruction overlay file and reloads the mind.
async fn execute_instruction(
    event: &EvolutionEvent,
    mind: &Arc<Mind>,
) -> Result<Value, AgentError> {
    let file = payload_str(event, "file")?;
    let new_content = payload_str(event, "new_content")?;

    // Validate that this is not a guardrail file.
    if sober_mind::is_guardrail_file(file, new_content) {
        return Err(AgentError::Internal(format!(
            "cannot modify guardrail instruction file: {file}"
        )));
    }

    mind.write_overlay(file, new_content)
        .map_err(|e| AgentError::Internal(format!("failed to write overlay: {e}")))?;

    info!(file = %file, "instruction overlay written");

    Ok(json!({}))
}

/// Creates a scheduled job via the scheduler gRPC service.
async fn execute_automation(
    event: &EvolutionEvent,
    scheduler_client: &SharedSchedulerClient,
) -> Result<Value, AgentError> {
    let job_name = payload_str(event, "job_name")?;
    let schedule = payload_str(event, "schedule")?;
    let prompt = payload_str(event, "prompt")?;
    let target_user_id_str = payload_str(event, "target_user_id")?;

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

    let job_payload = JobPayload::Prompt {
        text: prompt.to_owned(),
        workspace_id: None,
        model_hint: None,
    };
    let payload_bytes = serde_json::to_vec(&job_payload)
        .map_err(|e| AgentError::Internal(format!("failed to serialize job payload: {e}")))?;

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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extracts a required string field from the event payload.
fn payload_str<'a>(event: &'a EvolutionEvent, field: &str) -> Result<&'a str, AgentError> {
    event
        .payload
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| AgentError::Internal(format!("payload missing '{field}' field")))
}

/// Extracts an optional string array from the event payload.
fn payload_string_array(event: &EvolutionEvent, field: &str) -> Vec<String> {
    event
        .payload
        .get(field)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn skill_dir_resolves_correctly() {
        if let Ok(dir) = sober_skill::SkillLoader::resolve_skill_dir("test") {
            assert!(dir.to_string_lossy().ends_with(".sober/skills/test"));
        }
    }
}
