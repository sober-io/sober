//! Plugin registry — install, enable/disable, list, and uninstall plugins.
//!
//! The registry wraps the database-backed [`PluginRepo`] and runs each
//! install through the [`AuditPipeline`] before persisting.

use sober_core::error::AppError;
use sober_core::types::domain::{Plugin, PluginAuditLog};
use sober_core::types::enums::{PluginKind, PluginOrigin, PluginScope, PluginStatus};
use sober_core::types::ids::{PluginId, UserId, WorkspaceId};
use sober_core::types::input::{CreatePlugin, CreatePluginAuditLog, PluginFilter};
use sober_core::types::repo::PluginRepo;
use tracing::info;

use crate::audit::{AuditPipeline, AuditReport, AuditRequest, AuditVerdict};
use crate::error::PluginError;
use crate::manifest::PluginManifest;

/// Input for installing a new plugin via the registry.
#[derive(Debug, Clone)]
pub struct InstallRequest {
    /// Display name.
    pub name: String,
    /// Plugin kind (MCP, Skill, WASM).
    pub kind: PluginKind,
    /// Semantic version string.
    pub version: Option<String>,
    /// Human-readable description.
    pub description: Option<String>,
    /// How this plugin was discovered/installed.
    pub origin: PluginOrigin,
    /// Availability scope (system, user, workspace).
    pub scope: PluginScope,
    /// The user this plugin belongs to (for user-scoped plugins).
    pub owner_id: Option<UserId>,
    /// The workspace this plugin belongs to (for workspace-scoped plugins).
    pub workspace_id: Option<WorkspaceId>,
    /// Plugin-specific configuration (JSON).
    pub config: serde_json::Value,
    /// The user who triggered the install.
    pub installed_by: Option<UserId>,
    /// Parsed manifest (required for WASM plugins).
    pub manifest: Option<PluginManifest>,
    /// Raw WASM bytes.
    pub wasm_bytes: Option<Vec<u8>>,
}

/// The plugin registry.
///
/// Mediates between callers and the database, enforcing the audit pipeline
/// on every install.
pub struct PluginRegistry<P: PluginRepo> {
    db: P,
}

impl<P: PluginRepo> PluginRegistry<P> {
    /// Creates a new registry backed by the given repository.
    pub fn new(db: P) -> Self {
        Self { db }
    }

    /// Returns a reference to the underlying repository.
    ///
    /// Use for operations not covered by the registry API (e.g. `update_config`,
    /// `update_scope`, `get_kv_data`).
    pub fn repo(&self) -> &P {
        &self.db
    }

    /// Installs a plugin after running it through the audit pipeline.
    ///
    /// Returns the audit report. If approved, the plugin is persisted and
    /// the report's `plugin_name` field can be used to look it up.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::AlreadyExists`] if a plugin with the same
    /// name is already registered, or propagates database errors.
    pub async fn install(&self, request: InstallRequest) -> Result<AuditReport, PluginError> {
        // Check for duplicates within the same scope.
        let existing = self
            .db
            .list(PluginFilter {
                name: Some(request.name.clone()),
                scope: Some(request.scope),
                owner_id: request.owner_id,
                workspace_id: request.workspace_id,
                ..Default::default()
            })
            .await
            .map_err(|e| PluginError::Internal(e.into()))?;

        if !existing.is_empty() {
            return Err(PluginError::AlreadyExists(request.name));
        }

        // Run audit pipeline.
        let audit_request = AuditRequest {
            name: request.name.clone(),
            kind: request.kind,
            origin: request.origin,
            config: request.config.clone(),
            manifest: request.manifest,
            wasm_bytes: request.wasm_bytes,
        };
        let report = AuditPipeline::audit(&audit_request);

        // Persist the plugin if approved.
        let plugin_id = if report.verdict == AuditVerdict::Approved {
            let input = CreatePlugin {
                name: request.name.clone(),
                kind: request.kind,
                version: request.version,
                description: request.description,
                origin: request.origin,
                scope: request.scope,
                owner_id: request.owner_id,
                workspace_id: request.workspace_id,
                status: PluginStatus::Enabled,
                config: request.config,
                installed_by: request.installed_by,
            };
            let plugin = self
                .db
                .create(input)
                .await
                .map_err(|e| PluginError::Internal(e.into()))?;

            info!(plugin_id = %plugin.id, name = %plugin.name, "plugin installed");
            Some(plugin.id)
        } else {
            info!(name = %request.name, "plugin rejected by audit");
            None
        };

        // Store audit log.
        let (verdict_str, rejection_reason) = match &report.verdict {
            AuditVerdict::Approved => ("approved".to_string(), None),
            AuditVerdict::Rejected { reason, .. } => ("rejected".to_string(), Some(reason.clone())),
        };

        let stages_json =
            serde_json::to_value(&report.stages).map_err(|e| PluginError::Internal(e.into()))?;

        let audit_input = CreatePluginAuditLog {
            plugin_id,
            plugin_name: report.plugin_name.clone(),
            kind: report.plugin_kind,
            origin: report.origin,
            stages: stages_json,
            verdict: verdict_str,
            rejection_reason,
            audited_by: request.installed_by,
        };

        self.db
            .create_audit_log(audit_input)
            .await
            .map_err(|e| PluginError::Internal(e.into()))?;

        Ok(report)
    }

    /// Removes a plugin by ID.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::NotFound`] if the plugin does not exist,
    /// or propagates database errors.
    pub async fn uninstall(&self, id: PluginId) -> Result<(), PluginError> {
        // Verify the plugin exists first.
        let plugin = self.db.get_by_id(id).await.map_err(|e| match e {
            AppError::NotFound(msg) => PluginError::NotFound(msg),
            other => PluginError::Internal(other.into()),
        })?;

        self.db
            .delete(id)
            .await
            .map_err(|e| PluginError::Internal(e.into()))?;

        info!(plugin_id = %id, name = %plugin.name, "plugin uninstalled");
        Ok(())
    }

    /// Enables a plugin.
    ///
    /// # Errors
    ///
    /// Propagates database errors.
    pub async fn enable(&self, id: PluginId) -> Result<(), PluginError> {
        self.db
            .update_status(id, PluginStatus::Enabled)
            .await
            .map_err(|e| PluginError::Internal(e.into()))
    }

    /// Disables a plugin.
    ///
    /// # Errors
    ///
    /// Propagates database errors.
    pub async fn disable(&self, id: PluginId) -> Result<(), PluginError> {
        self.db
            .update_status(id, PluginStatus::Disabled)
            .await
            .map_err(|e| PluginError::Internal(e.into()))
    }

    /// Lists plugins matching the given filter.
    ///
    /// # Errors
    ///
    /// Propagates database errors.
    pub async fn list(&self, filter: PluginFilter) -> Result<Vec<Plugin>, PluginError> {
        self.db
            .list(filter)
            .await
            .map_err(|e| PluginError::Internal(e.into()))
    }

    /// Retrieves a plugin by ID.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::NotFound`] if the plugin does not exist.
    pub async fn get(&self, id: PluginId) -> Result<Plugin, PluginError> {
        self.db.get_by_id(id).await.map_err(|e| match e {
            AppError::NotFound(msg) => PluginError::NotFound(msg),
            other => PluginError::Internal(other.into()),
        })
    }

    /// Lists audit log entries for a plugin.
    ///
    /// # Errors
    ///
    /// Propagates database errors.
    pub async fn audit_logs(
        &self,
        plugin_id: PluginId,
        limit: i64,
    ) -> Result<Vec<PluginAuditLog>, PluginError> {
        self.db
            .list_audit_logs(plugin_id, limit)
            .await
            .map_err(|e| PluginError::Internal(e.into()))
    }
}
