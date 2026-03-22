//! Plugin- and skill-related gRPC handler logic, extracted from [`grpc`].
//!
//! These standalone async functions contain the implementation for plugin
//! management and skill RPCs. The `AgentService` trait impl in `grpc.rs`
//! delegates to them, keeping the main trait impl focused and the file
//! manageable.

use std::collections::HashSet;
use std::sync::Arc;

use sober_core::types::AgentRepos;
use sober_core::types::enums::{PluginKind, PluginScope, PluginStatus};
use sober_core::types::ids::{PluginId, WorkspaceId};
use sober_core::types::input::PluginFilter;
use sober_core::types::repo::{PluginRepo, WorkspaceRepo};
use sober_plugin::PluginManager;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use super::{AgentGrpcService, proto};

// ---------------------------------------------------------------------------
// JSON deserialization helper
// ---------------------------------------------------------------------------

/// JSON structure for an MCP server entry from `.mcp.json` format.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct McpServerEntry {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

/// Converts a domain [`Plugin`] to the proto [`PluginInfo`] representation.
pub(crate) fn plugin_to_proto(plugin: &sober_core::types::Plugin) -> proto::PluginInfo {
    proto::PluginInfo {
        id: plugin.id.to_string(),
        name: plugin.name.clone(),
        kind: format!("{:?}", plugin.kind).to_lowercase(),
        version: plugin.version.clone().unwrap_or_default(),
        description: plugin.description.clone().unwrap_or_default(),
        status: format!("{:?}", plugin.status).to_lowercase(),
        config: plugin.config.to_string(),
        installed_at: plugin.installed_at.to_rfc3339(),
        scope: format!("{:?}", plugin.scope).to_lowercase(),
    }
}

/// Parses a string into a [`PluginKind`] enum variant.
pub(crate) fn parse_plugin_kind(s: &str) -> Result<sober_core::types::PluginKind, String> {
    match s.to_lowercase().as_str() {
        "mcp" => Ok(sober_core::types::PluginKind::Mcp),
        "skill" => Ok(sober_core::types::PluginKind::Skill),
        "wasm" => Ok(sober_core::types::PluginKind::Wasm),
        other => Err(format!("unknown plugin kind: {other}")),
    }
}

/// Parses a string into a [`PluginStatus`] enum variant.
pub(crate) fn parse_plugin_status(s: &str) -> Result<sober_core::types::PluginStatus, String> {
    match s.to_lowercase().as_str() {
        "enabled" => Ok(sober_core::types::PluginStatus::Enabled),
        "disabled" => Ok(sober_core::types::PluginStatus::Disabled),
        "failed" => Ok(sober_core::types::PluginStatus::Failed),
        other => Err(format!("unknown plugin status: {other}")),
    }
}

/// Recursively copies a directory and its contents.
async fn copy_dir_recursive(
    src: &std::path::Path,
    dst: &std::path::Path,
) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type().await?.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Plugin RPC handlers
// ---------------------------------------------------------------------------

pub(crate) async fn handle_list_plugins<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    request: Request<proto::ListPluginsRequest>,
) -> Result<Response<proto::ListPluginsResponse>, Status> {
    let req = request.into_inner();

    let kind_filter = req
        .kind
        .map(|k| parse_plugin_kind(&k))
        .transpose()
        .map_err(|e| Status::invalid_argument(e.to_string()))?;

    let status_filter = req
        .status
        .map(|s| parse_plugin_status(&s))
        .transpose()
        .map_err(|e| Status::invalid_argument(e.to_string()))?;

    let filter = sober_core::types::PluginFilter {
        kind: kind_filter,
        status: status_filter,
        ..Default::default()
    };

    let plugins = service
        .agent()
        .repos()
        .plugins()
        .list(filter)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let plugin_infos = plugins.iter().map(plugin_to_proto).collect();

    Ok(Response::new(proto::ListPluginsResponse {
        plugins: plugin_infos,
    }))
}

pub(crate) async fn handle_install_plugin<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    request: Request<proto::InstallPluginRequest>,
) -> Result<Response<proto::InstallPluginResponse>, Status> {
    let req = request.into_inner();

    let kind = parse_plugin_kind(&req.kind).map_err(|e| Status::invalid_argument(e.to_string()))?;

    let config: serde_json::Value = serde_json::from_str(&req.config)
        .map_err(|e| Status::invalid_argument(format!("invalid config JSON: {e}")))?;

    let input = sober_core::types::CreatePlugin {
        name: req.name,
        kind,
        version: req.version,
        description: req.description,
        origin: sober_core::types::PluginOrigin::User,
        scope: sober_core::types::PluginScope::System,
        owner_id: None,
        workspace_id: None,
        status: sober_core::types::PluginStatus::Enabled,
        config,
        installed_by: None,
    };

    let plugin = service
        .agent()
        .repos()
        .plugins()
        .create(input)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    info!(plugin_id = %plugin.id, name = %plugin.name, "plugin installed");

    Ok(Response::new(proto::InstallPluginResponse {
        plugin: Some(plugin_to_proto(&plugin)),
    }))
}

pub(crate) async fn handle_uninstall_plugin<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    request: Request<proto::UninstallPluginRequest>,
) -> Result<Response<proto::UninstallPluginResponse>, Status> {
    let req = request.into_inner();

    let plugin_id = req
        .plugin_id
        .parse::<uuid::Uuid>()
        .map(PluginId::from_uuid)
        .map_err(|_| Status::invalid_argument("invalid plugin_id"))?;

    service
        .agent()
        .repos()
        .plugins()
        .delete(plugin_id)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    info!(%plugin_id, "plugin uninstalled");

    Ok(Response::new(proto::UninstallPluginResponse {}))
}

pub(crate) async fn handle_enable_plugin<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    request: Request<proto::EnablePluginRequest>,
) -> Result<Response<proto::EnablePluginResponse>, Status> {
    let req = request.into_inner();

    let plugin_id = req
        .plugin_id
        .parse::<uuid::Uuid>()
        .map(PluginId::from_uuid)
        .map_err(|_| Status::invalid_argument("invalid plugin_id"))?;

    service
        .agent()
        .repos()
        .plugins()
        .update_status(plugin_id, sober_core::types::PluginStatus::Enabled)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    info!(%plugin_id, "plugin enabled");

    Ok(Response::new(proto::EnablePluginResponse {}))
}

pub(crate) async fn handle_disable_plugin<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    request: Request<proto::DisablePluginRequest>,
) -> Result<Response<proto::DisablePluginResponse>, Status> {
    let req = request.into_inner();

    let plugin_id = req
        .plugin_id
        .parse::<uuid::Uuid>()
        .map(PluginId::from_uuid)
        .map_err(|_| Status::invalid_argument("invalid plugin_id"))?;

    service
        .agent()
        .repos()
        .plugins()
        .update_status(plugin_id, sober_core::types::PluginStatus::Disabled)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    info!(%plugin_id, "plugin disabled");

    Ok(Response::new(proto::DisablePluginResponse {}))
}

pub(crate) async fn handle_import_plugins<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    request: Request<proto::ImportPluginsRequest>,
) -> Result<Response<proto::ImportPluginsResponse>, Status> {
    let req = request.into_inner();

    let mcp_config: std::collections::HashMap<String, McpServerEntry> =
        serde_json::from_str(&req.mcp_servers_json)
            .map_err(|e| Status::invalid_argument(format!("invalid mcpServers JSON: {e}")))?;

    let mut imported = Vec::new();

    for (name, entry) in &mcp_config {
        let config = serde_json::json!({
            "command": entry.command,
            "args": entry.args,
            "env": entry.env,
        });

        let input = sober_core::types::CreatePlugin {
            name: name.clone(),
            kind: sober_core::types::PluginKind::Mcp,
            version: None,
            description: None,
            origin: sober_core::types::PluginOrigin::User,
            scope: sober_core::types::PluginScope::System,
            owner_id: None,
            workspace_id: None,
            status: sober_core::types::PluginStatus::Enabled,
            config,
            installed_by: None,
        };

        match service.agent().repos().plugins().create(input).await {
            Ok(plugin) => {
                info!(plugin_id = %plugin.id, name = %plugin.name, "MCP plugin imported");
                imported.push(plugin);
            }
            Err(e) => {
                warn!(name = %name, error = %e, "failed to import MCP plugin");
            }
        }
    }

    let plugin_infos = imported.iter().map(plugin_to_proto).collect();

    Ok(Response::new(proto::ImportPluginsResponse {
        imported_count: imported.len() as u32,
        plugins: plugin_infos,
    }))
}

pub(crate) async fn handle_reload_plugins<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    _request: Request<proto::ReloadPluginsRequest>,
) -> Result<Response<proto::ReloadPluginsResponse>, Status> {
    let filter = sober_core::types::PluginFilter {
        status: Some(sober_core::types::PluginStatus::Enabled),
        ..Default::default()
    };

    let active_plugins = service
        .agent()
        .repos()
        .plugins()
        .list(filter)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    info!(count = active_plugins.len(), "plugins reloaded");

    Ok(Response::new(proto::ReloadPluginsResponse {
        active_count: active_plugins.len() as u32,
    }))
}

pub(crate) async fn handle_change_plugin_scope<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    plugin_manager: &Arc<PluginManager<R::Plg>>,
    request: Request<proto::ChangePluginScopeRequest>,
) -> Result<Response<proto::ChangePluginScopeResponse>, Status> {
    let req = request.into_inner();

    let plugin_id = req
        .plugin_id
        .parse::<uuid::Uuid>()
        .map(PluginId::from_uuid)
        .map_err(|_| Status::invalid_argument("invalid plugin_id"))?;

    let new_scope = match req.new_scope.as_str() {
        "system" => sober_core::types::PluginScope::System,
        "user" => sober_core::types::PluginScope::User,
        "workspace" => sober_core::types::PluginScope::Workspace,
        other => return Err(Status::invalid_argument(format!("unknown scope: {other}"))),
    };

    let new_workspace_id = req
        .workspace_id
        .map(|s| {
            s.parse::<uuid::Uuid>()
                .map(WorkspaceId::from_uuid)
                .map_err(|_| Status::invalid_argument("invalid workspace_id"))
        })
        .transpose()?;

    // Fetch the plugin.
    let plugin = service
        .agent()
        .repos()
        .plugins()
        .get_by_id(plugin_id)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    // Move files on disk for skill plugins.
    if plugin.kind == sober_core::types::PluginKind::Skill
        && let Some(src_path) = plugin.config.get("path").and_then(|v| v.as_str())
    {
        let src = std::path::Path::new(src_path);

        // Determine destination directory.
        let user_home = sober_workspace::user_home_dir();
        let dest_dir = match new_scope {
            sober_core::types::PluginScope::User => user_home.join(".sober").join("skills"),
            sober_core::types::PluginScope::Workspace => {
                // Resolve workspace root from workspace_id.
                let ws_id = new_workspace_id.ok_or_else(|| {
                    Status::invalid_argument("workspace_id required when moving to workspace scope")
                })?;
                let ws = service
                    .agent()
                    .repos()
                    .workspaces()
                    .get_by_id(ws_id)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;
                std::path::PathBuf::from(&ws.root_path)
                    .join(".sober")
                    .join("skills")
            }
            sober_core::types::PluginScope::System => {
                return Err(Status::invalid_argument(
                    "cannot move skill files to system scope (system skills are compiled in)",
                ));
            }
        };

        if src.exists() {
            // Determine if src is a file (skill.md) or directory (skill-name/).
            let (dest_path, new_config_path) = if src.is_dir() {
                let dir_name = src
                    .file_name()
                    .ok_or_else(|| Status::internal("invalid source path"))?;
                let dest = dest_dir.join(dir_name);
                let config_path = dest.join("SKILL.md").to_string_lossy().to_string();
                (dest, config_path)
            } else {
                let file_name = src
                    .file_name()
                    .ok_or_else(|| Status::internal("invalid source path"))?;
                let dest = dest_dir.join(file_name);
                let config_path = dest.to_string_lossy().to_string();
                (dest, config_path)
            };

            // Create destination directory.
            tokio::fs::create_dir_all(&dest_dir).await.map_err(|e| {
                Status::internal(format!("failed to create destination directory: {e}"))
            })?;

            // Move: copy source to dest, then remove source.
            if src.is_dir() {
                copy_dir_recursive(src, &dest_path).await.map_err(|e| {
                    Status::internal(format!("failed to copy skill directory: {e}"))
                })?;
                tokio::fs::remove_dir_all(src).await.map_err(|e| {
                    Status::internal(format!("failed to remove source directory: {e}"))
                })?;
            } else {
                tokio::fs::copy(src, &dest_path)
                    .await
                    .map_err(|e| Status::internal(format!("failed to copy skill file: {e}")))?;
                tokio::fs::remove_file(src)
                    .await
                    .map_err(|e| Status::internal(format!("failed to remove source file: {e}")))?;
            }

            // Update config with new path.
            let mut config = plugin.config.clone();
            config["path"] = serde_json::Value::String(new_config_path);
            config["source"] = serde_json::Value::String(format!("{new_scope:?}"));
            service
                .agent()
                .repos()
                .plugins()
                .update_config(plugin_id, config)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        }
    }
    // Update scope and workspace_id in DB.
    service
        .agent()
        .repos()
        .plugins()
        .update_scope(plugin_id, new_scope)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    // Invalidate skill cache so next load picks up the new location.
    plugin_manager.skill_loader().invalidate_cache();

    // Re-fetch and return.
    let updated = service
        .agent()
        .repos()
        .plugins()
        .get_by_id(plugin_id)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    info!(
        %plugin_id,
        name = %updated.name,
        scope = ?new_scope,
        "plugin scope changed"
    );

    Ok(Response::new(proto::ChangePluginScopeResponse {
        plugin: Some(plugin_to_proto(&updated)),
    }))
}

// ---------------------------------------------------------------------------
// Skill RPC handlers
// ---------------------------------------------------------------------------

pub(crate) async fn handle_list_skills<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    request: Request<proto::ListSkillsRequest>,
) -> Result<Response<proto::ListSkillsResponse>, Status> {
    let req = request.into_inner();
    let ws = service
        .resolve_workspace_context(req.conversation_id.as_deref())
        .await;

    // Database is the source of truth. Return enabled skill plugins.
    let filter = PluginFilter {
        kind: Some(PluginKind::Skill),
        status: Some(PluginStatus::Enabled),
        ..Default::default()
    };

    let plugins = service
        .agent()
        .repos()
        .plugins()
        .list(filter)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    // Filter: include system + user scoped skills always,
    // workspace-scoped only if they match the current workspace.
    // Deduplicate by name (first occurrence wins).
    let mut seen = HashSet::new();
    let skills = plugins
        .into_iter()
        .filter(|p| match p.scope {
            PluginScope::System | PluginScope::User => true,
            PluginScope::Workspace => match (p.workspace_id, ws.workspace_id) {
                (Some(pw), Some(cw)) => pw == cw,
                _ => false,
            },
        })
        .filter(|p| seen.insert(p.name.clone()))
        .map(|p| proto::SkillInfo {
            name: p.name,
            description: p.description.unwrap_or_default(),
        })
        .collect();

    Ok(Response::new(proto::ListSkillsResponse { skills }))
}

pub(crate) async fn handle_reload_skills<R: AgentRepos>(
    service: &AgentGrpcService<R>,
    plugin_manager: &Arc<PluginManager<R::Plg>>,
    request: Request<proto::ReloadSkillsRequest>,
) -> Result<Response<proto::ReloadSkillsResponse>, Status> {
    let req = request.into_inner();

    // Invalidate skill cache so next load re-discovers from disk.
    plugin_manager.skill_loader().invalidate_cache();

    let ws = service
        .resolve_workspace_context(req.conversation_id.as_deref())
        .await;

    // Re-sync filesystem skills into the DB.
    let user_home = sober_workspace::user_home_dir();
    let _ = plugin_manager
        .sync_filesystem_skills(&user_home, &ws.workspace_dir, ws.workspace_id)
        .await;

    // Return enabled skills from the database, filtered by workspace scope.
    let filter = PluginFilter {
        kind: Some(PluginKind::Skill),
        status: Some(PluginStatus::Enabled),
        ..Default::default()
    };

    let plugins = service
        .agent()
        .repos()
        .plugins()
        .list(filter)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let mut seen = HashSet::new();
    let skills = plugins
        .into_iter()
        .filter(|p| match p.scope {
            PluginScope::System | PluginScope::User => true,
            PluginScope::Workspace => match (p.workspace_id, ws.workspace_id) {
                (Some(pw), Some(cw)) => pw == cw,
                _ => false,
            },
        })
        .filter(|p| seen.insert(p.name.clone()))
        .map(|p| proto::SkillInfo {
            name: p.name,
            description: p.description.unwrap_or_default(),
        })
        .collect();

    Ok(Response::new(proto::ReloadSkillsResponse { skills }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sober_core::types::ids::PluginId;

    #[test]
    fn parse_plugin_kind_valid() {
        assert_eq!(
            parse_plugin_kind("mcp").unwrap(),
            sober_core::types::PluginKind::Mcp
        );
        assert_eq!(
            parse_plugin_kind("MCP").unwrap(),
            sober_core::types::PluginKind::Mcp
        );
        assert_eq!(
            parse_plugin_kind("skill").unwrap(),
            sober_core::types::PluginKind::Skill
        );
        assert_eq!(
            parse_plugin_kind("wasm").unwrap(),
            sober_core::types::PluginKind::Wasm
        );
    }

    #[test]
    fn parse_plugin_kind_invalid() {
        assert!(parse_plugin_kind("unknown").is_err());
        assert!(parse_plugin_kind("").is_err());
    }

    #[test]
    fn parse_plugin_status_valid() {
        assert_eq!(
            parse_plugin_status("enabled").unwrap(),
            sober_core::types::PluginStatus::Enabled
        );
        assert_eq!(
            parse_plugin_status("DISABLED").unwrap(),
            sober_core::types::PluginStatus::Disabled
        );
        assert_eq!(
            parse_plugin_status("failed").unwrap(),
            sober_core::types::PluginStatus::Failed
        );
    }

    #[test]
    fn parse_plugin_status_invalid() {
        assert!(parse_plugin_status("unknown").is_err());
        assert!(parse_plugin_status("").is_err());
    }

    #[test]
    fn plugin_to_proto_conversion() {
        let plugin = sober_core::types::Plugin {
            id: PluginId::new(),
            name: "test-plugin".to_owned(),
            kind: sober_core::types::PluginKind::Mcp,
            version: Some("1.0.0".to_owned()),
            description: Some("A test plugin".to_owned()),
            origin: sober_core::types::PluginOrigin::User,
            scope: sober_core::types::PluginScope::System,
            owner_id: None,
            workspace_id: None,
            status: sober_core::types::PluginStatus::Enabled,
            config: serde_json::json!({"command": "test"}),
            installed_by: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let proto_info = plugin_to_proto(&plugin);

        assert_eq!(proto_info.id, plugin.id.to_string());
        assert_eq!(proto_info.name, "test-plugin");
        assert_eq!(proto_info.kind, "mcp");
        assert_eq!(proto_info.version, "1.0.0");
        assert_eq!(proto_info.description, "A test plugin");
        assert_eq!(proto_info.status, "enabled");
        assert!(proto_info.config.contains("command"));
        assert!(!proto_info.installed_at.is_empty());
    }

    #[test]
    fn plugin_to_proto_defaults() {
        let plugin = sober_core::types::Plugin {
            id: PluginId::new(),
            name: "minimal".to_owned(),
            kind: sober_core::types::PluginKind::Wasm,
            version: None,
            description: None,
            origin: sober_core::types::PluginOrigin::Agent,
            scope: sober_core::types::PluginScope::User,
            owner_id: None,
            workspace_id: None,
            status: sober_core::types::PluginStatus::Disabled,
            config: serde_json::json!({}),
            installed_by: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let proto_info = plugin_to_proto(&plugin);

        assert_eq!(proto_info.kind, "wasm");
        assert_eq!(proto_info.version, "");
        assert_eq!(proto_info.description, "");
        assert_eq!(proto_info.status, "disabled");
    }

    #[test]
    fn mcp_server_entry_deserialization() {
        let json = r#"{"test-server": {"command": "node", "args": ["server.js"], "env": {"PORT": "3000"}}}"#;
        let parsed: std::collections::HashMap<String, McpServerEntry> =
            serde_json::from_str(json).expect("valid JSON");

        assert_eq!(parsed.len(), 1);
        let entry = parsed.get("test-server").expect("entry exists");
        assert_eq!(entry.command, "node");
        assert_eq!(entry.args, vec!["server.js"]);
        assert_eq!(entry.env.get("PORT").unwrap(), "3000");
    }

    #[test]
    fn mcp_server_entry_defaults() {
        let json = r#"{"minimal": {"command": "echo"}}"#;
        let parsed: std::collections::HashMap<String, McpServerEntry> =
            serde_json::from_str(json).expect("valid JSON");

        let entry = parsed.get("minimal").expect("entry exists");
        assert_eq!(entry.command, "echo");
        assert!(entry.args.is_empty());
        assert!(entry.env.is_empty());
    }
}
