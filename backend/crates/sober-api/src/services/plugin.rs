use serde::Serialize;
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{PluginId, PluginRepo, UserId};
use sober_db::PgPluginRepo;
use sqlx::PgPool;

use crate::guards;
use crate::proto;
use crate::state::AgentClient;

/// Typed response for a plugin.
#[derive(Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub version: String,
    pub description: String,
    pub status: String,
    pub scope: String,
    pub owner_id: Option<String>,
    pub config: serde_json::Value,
    pub installed_at: String,
}

impl From<proto::PluginInfo> for PluginInfo {
    fn from(info: proto::PluginInfo) -> Self {
        let config: serde_json::Value =
            serde_json::from_str(&info.config).unwrap_or(serde_json::json!({}));
        Self {
            id: info.id,
            name: info.name,
            kind: info.kind,
            version: info.version,
            description: info.description,
            status: info.status,
            scope: info.scope,
            owner_id: info.owner_id,
            config,
            installed_at: info.installed_at,
        }
    }
}

/// Typed response for plugin import.
#[derive(Serialize)]
pub struct ImportResult {
    pub imported_count: u32,
    pub plugins: Vec<PluginInfo>,
}

/// Typed response for plugin reload.
#[derive(Serialize)]
pub struct ReloadResult {
    pub active_count: u32,
}

/// Typed response for a skill.
#[derive(Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
}

/// Typed response for a tool.
#[derive(Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
}

/// Typed response for an audit log entry.
#[derive(Serialize)]
pub struct AuditLogEntry {
    pub id: String,
    pub plugin_id: Option<String>,
    pub plugin_name: Option<String>,
    pub kind: String,
    pub origin: String,
    pub stages: serde_json::Value,
    pub verdict: String,
    pub rejection_reason: Option<String>,
    pub audited_at: String,
    pub audited_by: Option<String>,
}

pub struct PluginService {
    db: PgPool,
    agent_client: AgentClient,
}

impl PluginService {
    pub fn new(db: PgPool, agent_client: AgentClient) -> Self {
        Self { db, agent_client }
    }

    /// List plugins with optional filters.
    pub async fn list(
        &self,
        kind: Option<String>,
        status: Option<String>,
        workspace_id: Option<String>,
    ) -> Result<Vec<PluginInfo>, AppError> {
        let mut client = self.agent_client.clone();
        let response = client
            .list_plugins(proto::ListPluginsRequest {
                kind,
                status,
                workspace_id,
            })
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(response
            .into_inner()
            .plugins
            .into_iter()
            .map(PluginInfo::from)
            .collect())
    }

    /// Install a new plugin.
    pub async fn install(
        &self,
        name: String,
        kind: String,
        config: serde_json::Value,
        description: Option<String>,
        version: Option<String>,
    ) -> Result<Option<PluginInfo>, AppError> {
        let mut client = self.agent_client.clone();
        let config_str =
            serde_json::to_string(&config).map_err(|e| AppError::Internal(e.into()))?;

        let response = client
            .install_plugin(proto::InstallPluginRequest {
                name,
                kind,
                config: config_str,
                description,
                version,
            })
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(response.into_inner().plugin.map(PluginInfo::from))
    }

    /// Batch import plugins from `.mcp.json` config.
    pub async fn import(&self, mcp_servers: serde_json::Value) -> Result<ImportResult, AppError> {
        let mut client = self.agent_client.clone();
        let mcp_servers_json =
            serde_json::to_string(&mcp_servers).map_err(|e| AppError::Internal(e.into()))?;

        let response = client
            .import_plugins(proto::ImportPluginsRequest { mcp_servers_json })
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let inner = response.into_inner();
        Ok(ImportResult {
            imported_count: inner.imported_count,
            plugins: inner.plugins.into_iter().map(PluginInfo::from).collect(),
        })
    }

    /// Reload all plugins.
    pub async fn reload(&self, user: &AuthUser) -> Result<ReloadResult, AppError> {
        guards::require_admin(user)?;
        let mut client = self.agent_client.clone();
        let response = client
            .reload_plugins(proto::ReloadPluginsRequest {})
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(ReloadResult {
            active_count: response.into_inner().active_count,
        })
    }

    /// Get a single plugin by ID.
    pub async fn get(&self, id: uuid::Uuid) -> Result<PluginInfo, AppError> {
        let mut client = self.agent_client.clone();
        let response = client
            .list_plugins(proto::ListPluginsRequest {
                kind: None,
                status: None,
                workspace_id: None,
            })
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let id_str = id.to_string();
        response
            .into_inner()
            .plugins
            .into_iter()
            .find(|p| p.id == id_str)
            .map(PluginInfo::from)
            .ok_or_else(|| AppError::NotFound("plugin".into()))
    }

    /// Update a plugin (enable/disable, config, scope).
    pub async fn update(
        &self,
        id: uuid::Uuid,
        user: &AuthUser,
        enabled: Option<bool>,
        config: Option<serde_json::Value>,
        scope: Option<String>,
    ) -> Result<PluginInfo, AppError> {
        let mut client = self.agent_client.clone();
        let plugin_id = PluginId::from_uuid(id);
        let repo = PgPluginRepo::new(self.db.clone());
        let plugin = repo.get_by_id(plugin_id).await?;
        guards::can_modify_plugin(user, &plugin)?;
        let id_str = id.to_string();

        if let Some(enabled) = enabled {
            if enabled {
                client
                    .enable_plugin(proto::EnablePluginRequest {
                        plugin_id: id_str.clone(),
                    })
                    .await
                    .map_err(|e| AppError::Internal(e.into()))?;
            } else {
                client
                    .disable_plugin(proto::DisablePluginRequest {
                        plugin_id: id_str.clone(),
                    })
                    .await
                    .map_err(|e| AppError::Internal(e.into()))?;
            }
        }

        if let Some(config) = config {
            repo.update_config(plugin_id, config).await?;
        }

        if let Some(scope_str) = scope {
            client
                .change_plugin_scope(proto::ChangePluginScopeRequest {
                    plugin_id: id_str.clone(),
                    new_scope: scope_str,
                    workspace_id: None,
                })
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }

        // Re-fetch via gRPC.
        let response = client
            .list_plugins(proto::ListPluginsRequest {
                kind: None,
                status: None,
                workspace_id: None,
            })
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        response
            .into_inner()
            .plugins
            .into_iter()
            .find(|p| p.id == id_str)
            .map(PluginInfo::from)
            .ok_or_else(|| AppError::NotFound("plugin".into()))
    }

    /// Uninstall a plugin.
    pub async fn uninstall(&self, id: uuid::Uuid, user: &AuthUser) -> Result<(), AppError> {
        let plugin_id = PluginId::from_uuid(id);
        let repo = PgPluginRepo::new(self.db.clone());
        let plugin = repo.get_by_id(plugin_id).await?;
        guards::can_modify_plugin(user, &plugin)?;
        let mut client = self.agent_client.clone();
        client
            .uninstall_plugin(proto::UninstallPluginRequest {
                plugin_id: id.to_string(),
            })
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        Ok(())
    }

    /// List audit log entries for a plugin.
    pub async fn list_audit_logs(
        &self,
        id: uuid::Uuid,
        limit: i64,
    ) -> Result<Vec<AuditLogEntry>, AppError> {
        let repo = PgPluginRepo::new(self.db.clone());
        let plugin_id = PluginId::from_uuid(id);
        let logs = repo.list_audit_logs(plugin_id, limit).await?;

        Ok(logs
            .into_iter()
            .map(|log| AuditLogEntry {
                id: log.id.to_string(),
                plugin_id: log.plugin_id.map(|id| id.to_string()),
                plugin_name: Some(log.plugin_name),
                kind: serde_json::to_string(&log.kind)
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string(),
                origin: serde_json::to_string(&log.origin)
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string(),
                stages: log.stages,
                verdict: log.verdict,
                rejection_reason: log.rejection_reason,
                audited_at: log.audited_at.to_rfc3339(),
                audited_by: log.audited_by.map(|id| id.to_string()),
            })
            .collect())
    }

    /// List available skills.
    pub async fn list_skills(
        &self,
        user_id: UserId,
        conversation_id: Option<String>,
    ) -> Result<Vec<SkillInfo>, AppError> {
        let mut client = self.agent_client.clone();
        let response = client
            .list_skills(proto::ListSkillsRequest {
                user_id: user_id.to_string(),
                conversation_id,
            })
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(response
            .into_inner()
            .skills
            .into_iter()
            .map(|s| SkillInfo {
                name: s.name,
                description: s.description,
            })
            .collect())
    }

    /// Reload skills and return fresh list.
    pub async fn reload_skills(
        &self,
        user_id: UserId,
        conversation_id: Option<String>,
    ) -> Result<Vec<SkillInfo>, AppError> {
        let mut client = self.agent_client.clone();
        let response = client
            .reload_skills(proto::ReloadSkillsRequest {
                conversation_id,
                user_id: Some(user_id.to_string()),
            })
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(response
            .into_inner()
            .skills
            .into_iter()
            .map(|s| SkillInfo {
                name: s.name,
                description: s.description,
            })
            .collect())
    }

    /// List all available tools.
    pub async fn list_tools(&self) -> Result<Vec<ToolInfo>, AppError> {
        let mut client = self.agent_client.clone();
        let response = client
            .list_tools(proto::ListToolsRequest {
                user_id: String::new(),
                workspace_id: None,
            })
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(response
            .into_inner()
            .tools
            .into_iter()
            .map(|t| ToolInfo {
                name: t.name,
                description: t.description,
                source: t.source,
                plugin_id: t.plugin_id,
                plugin_name: t.plugin_name,
            })
            .collect())
    }
}
