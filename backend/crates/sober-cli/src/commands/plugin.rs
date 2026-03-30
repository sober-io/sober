//! Plugin management commands.
//!
//! Reads plugin state from the database and triggers enable/disable via
//! the agent gRPC service over Unix domain socket.

use anyhow::{Context, Result};
use sober_core::types::{PluginFilter, PluginId, PluginKind, PluginRepo, PluginStatus};
use sober_db::PgPluginRepo;

use super::common::{connect_agent, proto};
use proto::{DisablePluginRequest, EnablePluginRequest};

/// List installed plugins, optionally filtered by kind and status.
pub async fn list(
    repo: &PgPluginRepo,
    kind_filter: Option<String>,
    status_filter: Option<String>,
) -> Result<()> {
    let kind = kind_filter
        .map(|k| parse_plugin_kind(&k))
        .transpose()
        .context("invalid --kind filter")?;

    let status = status_filter
        .map(|s| parse_plugin_status(&s))
        .transpose()
        .context("invalid --status filter")?;

    let filter = PluginFilter {
        kind,
        status,
        ..Default::default()
    };

    let plugins = repo.list(filter).await.context("failed to list plugins")?;

    if plugins.is_empty() {
        println!("No plugins found.");
        return Ok(());
    }

    println!(
        "{:<8} {:<30} {:<12} {:<10} {:<10} INSTALLED",
        "KIND", "NAME", "SCOPE", "STATUS", "ORIGIN",
    );
    println!("{}", "-".repeat(100));

    for plugin in &plugins {
        println!(
            "{:<8} {:<30} {:<12} {:<10} {:<10} {}",
            format_plugin_kind(plugin.kind),
            truncate(&plugin.name, 28),
            format_plugin_scope(plugin.scope),
            format_plugin_status(plugin.status),
            format_plugin_origin(plugin.origin),
            plugin.installed_at.format("%Y-%m-%d %H:%M"),
        );
    }

    println!("\n{} plugin(s) total.", plugins.len());
    Ok(())
}

/// Enable a disabled plugin via the agent.
pub async fn enable(repo: &PgPluginRepo, id: &str, socket: &str) -> Result<()> {
    let plugin_id = parse_uuid(id)?;

    let plugin = repo
        .get_by_id(plugin_id)
        .await
        .context("failed to find plugin")?;

    if plugin.status == PluginStatus::Enabled {
        anyhow::bail!("plugin '{}' is already enabled", plugin.name);
    }

    let mut client = connect_agent(socket).await?;
    client
        .enable_plugin(EnablePluginRequest {
            plugin_id: id.to_owned(),
        })
        .await
        .context("failed to enable plugin via agent")?;

    println!("Enabled plugin: {}", plugin.name);
    Ok(())
}

/// Disable an active plugin via the agent.
pub async fn disable(repo: &PgPluginRepo, id: &str, socket: &str) -> Result<()> {
    let plugin_id = parse_uuid(id)?;

    let plugin = repo
        .get_by_id(plugin_id)
        .await
        .context("failed to find plugin")?;

    if plugin.status == PluginStatus::Disabled {
        anyhow::bail!("plugin '{}' is already disabled", plugin.name);
    }

    let mut client = connect_agent(socket).await?;
    client
        .disable_plugin(DisablePluginRequest {
            plugin_id: id.to_owned(),
        })
        .await
        .context("failed to disable plugin via agent")?;

    println!("Disabled plugin: {}", plugin.name);
    Ok(())
}

/// Remove a plugin entirely.
pub async fn remove(repo: &PgPluginRepo, id: &str) -> Result<()> {
    let plugin_id = parse_uuid(id)?;

    let plugin = repo
        .get_by_id(plugin_id)
        .await
        .context("failed to find plugin")?;

    repo.delete(plugin_id)
        .await
        .context("failed to delete plugin")?;

    println!("Removed plugin: {} ({})", plugin.name, plugin.id);
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Parse a UUID string into a `PluginId`.
fn parse_uuid(id: &str) -> Result<PluginId> {
    let uuid = sober_core::Uuid::parse_str(id).with_context(|| format!("invalid UUID: {id}"))?;
    Ok(PluginId::from_uuid(uuid))
}

/// Parse a string into a `PluginKind`.
fn parse_plugin_kind(s: &str) -> Result<PluginKind> {
    match s.to_lowercase().as_str() {
        "mcp" => Ok(PluginKind::Mcp),
        "skill" => Ok(PluginKind::Skill),
        "wasm" => Ok(PluginKind::Wasm),
        _ => anyhow::bail!("unknown plugin kind '{s}' — expected: mcp, skill, wasm"),
    }
}

/// Parse a string into a `PluginStatus`.
fn parse_plugin_status(s: &str) -> Result<PluginStatus> {
    match s.to_lowercase().as_str() {
        "enabled" => Ok(PluginStatus::Enabled),
        "disabled" => Ok(PluginStatus::Disabled),
        "failed" => Ok(PluginStatus::Failed),
        _ => anyhow::bail!("unknown plugin status '{s}' — expected: enabled, disabled, failed"),
    }
}

/// Format a `PluginKind` as a lowercase string.
fn format_plugin_kind(k: PluginKind) -> &'static str {
    match k {
        PluginKind::Mcp => "mcp",
        PluginKind::Skill => "skill",
        PluginKind::Wasm => "wasm",
    }
}

/// Format a `PluginScope` as a lowercase string.
fn format_plugin_scope(s: sober_core::types::PluginScope) -> &'static str {
    match s {
        sober_core::types::PluginScope::System => "system",
        sober_core::types::PluginScope::User => "user",
        sober_core::types::PluginScope::Workspace => "workspace",
    }
}

/// Format a `PluginStatus` as a lowercase string.
fn format_plugin_status(s: PluginStatus) -> &'static str {
    match s {
        PluginStatus::Enabled => "enabled",
        PluginStatus::Disabled => "disabled",
        PluginStatus::Failed => "failed",
    }
}

/// Format a `PluginOrigin` as a lowercase string.
fn format_plugin_origin(o: sober_core::types::PluginOrigin) -> &'static str {
    match o {
        sober_core::types::PluginOrigin::System => "system",
        sober_core::types::PluginOrigin::Agent => "agent",
        sober_core::types::PluginOrigin::User => "user",
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
