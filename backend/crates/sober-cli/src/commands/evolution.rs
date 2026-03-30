//! Evolution management commands.
//!
//! Reads/writes evolution events via the database and triggers execution
//! or revert via the agent gRPC service over Unix domain socket.

use anyhow::{Context, Result};
use sober_core::types::{EvolutionEventId, EvolutionRepo, EvolutionStatus, EvolutionType};
use sober_db::PgEvolutionRepo;

use super::common::{connect_agent, proto};
use proto::{ExecuteEvolutionRequest, RevertEvolutionRequest};

/// List evolution events, optionally filtered by type and status.
pub async fn list(
    repo: &PgEvolutionRepo,
    type_filter: Option<String>,
    status_filter: Option<String>,
) -> Result<()> {
    let evo_type = type_filter
        .map(|t| parse_evolution_type(&t))
        .transpose()
        .context("invalid --type filter")?;

    let evo_status = status_filter
        .map(|s| parse_evolution_status(&s))
        .transpose()
        .context("invalid --status filter")?;

    let events = repo
        .list(evo_type, evo_status)
        .await
        .context("failed to list evolution events")?;

    if events.is_empty() {
        println!("No evolution events found.");
        return Ok(());
    }

    println!(
        "{:<38} {:<14} {:<40} {:<10} {:<6} CREATED",
        "ID", "TYPE", "TITLE", "STATUS", "CONF",
    );
    println!("{}", "-".repeat(120));

    for event in &events {
        println!(
            "{:<38} {:<14} {:<40} {:<10} {:<6.2} {}",
            event.id,
            format_evolution_type(event.evolution_type),
            truncate(&event.title, 38),
            format_evolution_status(event.status),
            event.confidence,
            event.created_at.format("%Y-%m-%d %H:%M"),
        );
    }

    println!("\n{} event(s) total.", events.len());
    Ok(())
}

/// Approve a proposed evolution event and trigger immediate execution via the agent.
pub async fn approve(repo: &PgEvolutionRepo, id: &str, socket: &str) -> Result<()> {
    let event_id = parse_uuid(id)?;

    // Verify the event exists and is in proposed state.
    let event = repo
        .get_by_id(event_id)
        .await
        .context("failed to find evolution event")?;

    if event.status != EvolutionStatus::Proposed {
        anyhow::bail!(
            "event {} has status '{}', expected 'proposed'",
            id,
            format_evolution_status(event.status),
        );
    }

    // Update status to approved in the DB.
    repo.update_status(event_id, EvolutionStatus::Approved, None)
        .await
        .context("failed to approve evolution event")?;

    println!("Approved evolution event: {}", event.title);

    // Trigger execution via agent gRPC.
    let mut client = connect_agent(socket).await?;
    let resp = client
        .execute_evolution(ExecuteEvolutionRequest {
            evolution_event_id: id.to_owned(),
        })
        .await
        .context("failed to trigger evolution execution")?;

    let inner = resp.into_inner();
    if inner.success {
        println!("Execution triggered successfully.");
    } else {
        println!("Execution trigger failed: {}", inner.error);
    }

    Ok(())
}

/// Reject a proposed evolution event.
pub async fn reject(repo: &PgEvolutionRepo, id: &str) -> Result<()> {
    let event_id = parse_uuid(id)?;

    let event = repo
        .get_by_id(event_id)
        .await
        .context("failed to find evolution event")?;

    if event.status != EvolutionStatus::Proposed {
        anyhow::bail!(
            "event {} has status '{}', expected 'proposed'",
            id,
            format_evolution_status(event.status),
        );
    }

    repo.update_status(event_id, EvolutionStatus::Rejected, None)
        .await
        .context("failed to reject evolution event")?;

    println!("Rejected evolution event: {}", event.title);
    Ok(())
}

/// Revert an active evolution event via the agent.
pub async fn revert(repo: &PgEvolutionRepo, id: &str, socket: &str) -> Result<()> {
    let event_id = parse_uuid(id)?;

    let event = repo
        .get_by_id(event_id)
        .await
        .context("failed to find evolution event")?;

    if event.status != EvolutionStatus::Active {
        anyhow::bail!(
            "event {} has status '{}', expected 'active'",
            id,
            format_evolution_status(event.status),
        );
    }

    // Update status to reverted in the DB.
    repo.update_status(event_id, EvolutionStatus::Reverted, None)
        .await
        .context("failed to update evolution event status")?;

    println!("Reverting evolution event: {}", event.title);

    // Trigger revert via agent gRPC.
    let mut client = connect_agent(socket).await?;
    let resp = client
        .revert_evolution(RevertEvolutionRequest {
            evolution_event_id: id.to_owned(),
        })
        .await
        .context("failed to trigger evolution revert")?;

    let inner = resp.into_inner();
    if inner.success {
        println!("Revert completed successfully.");
    } else {
        println!("Revert failed: {}", inner.error);
    }

    Ok(())
}

/// Show current evolution autonomy configuration.
pub async fn config(repo: &PgEvolutionRepo) -> Result<()> {
    let cfg = repo
        .get_config()
        .await
        .context("failed to load evolution config")?;

    let app_config = sober_core::config::AppConfig::load().context("failed to load app config")?;

    println!("Evolution Configuration");
    println!("{}", "-".repeat(50));
    println!(
        "  Check interval:          {}",
        app_config.evolution.interval
    );
    println!("  Plugin autonomy:         {:?}", cfg.plugin_autonomy);
    println!("  Skill autonomy:          {:?}", cfg.skill_autonomy);
    println!("  Instruction autonomy:    {:?}", cfg.instruction_autonomy);
    println!("  Automation autonomy:     {:?}", cfg.automation_autonomy);
    println!(
        "  Last updated:            {}",
        cfg.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
    );

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Parse a UUID string into an `EvolutionEventId`.
fn parse_uuid(id: &str) -> Result<EvolutionEventId> {
    let uuid = sober_core::Uuid::parse_str(id).with_context(|| format!("invalid UUID: {id}"))?;
    Ok(EvolutionEventId::from_uuid(uuid))
}

/// Parse a string into an `EvolutionType`.
fn parse_evolution_type(s: &str) -> Result<EvolutionType> {
    match s.to_lowercase().as_str() {
        "plugin" => Ok(EvolutionType::Plugin),
        "skill" => Ok(EvolutionType::Skill),
        "instruction" => Ok(EvolutionType::Instruction),
        "automation" => Ok(EvolutionType::Automation),
        _ => anyhow::bail!(
            "unknown evolution type '{s}' — expected: plugin, skill, instruction, automation"
        ),
    }
}

/// Parse a string into an `EvolutionStatus`.
fn parse_evolution_status(s: &str) -> Result<EvolutionStatus> {
    match s.to_lowercase().as_str() {
        "proposed" => Ok(EvolutionStatus::Proposed),
        "approved" => Ok(EvolutionStatus::Approved),
        "executing" => Ok(EvolutionStatus::Executing),
        "active" => Ok(EvolutionStatus::Active),
        "failed" => Ok(EvolutionStatus::Failed),
        "rejected" => Ok(EvolutionStatus::Rejected),
        "reverted" => Ok(EvolutionStatus::Reverted),
        _ => anyhow::bail!(
            "unknown evolution status '{s}' — expected: proposed, approved, executing, active, failed, rejected, reverted"
        ),
    }
}

/// Format an `EvolutionType` as a lowercase string.
fn format_evolution_type(t: EvolutionType) -> &'static str {
    match t {
        EvolutionType::Plugin => "plugin",
        EvolutionType::Skill => "skill",
        EvolutionType::Instruction => "instruction",
        EvolutionType::Automation => "automation",
    }
}

/// Format an `EvolutionStatus` as a lowercase string.
fn format_evolution_status(s: EvolutionStatus) -> &'static str {
    match s {
        EvolutionStatus::Proposed => "proposed",
        EvolutionStatus::Approved => "approved",
        EvolutionStatus::Executing => "executing",
        EvolutionStatus::Active => "active",
        EvolutionStatus::Failed => "failed",
        EvolutionStatus::Rejected => "rejected",
        EvolutionStatus::Reverted => "reverted",
    }
}

/// Truncate a string to `max` chars, appending "..." if truncated.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}
