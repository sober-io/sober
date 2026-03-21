//! Skill plugin cleanup executor — removes plugin entries whose skill files
//! no longer exist on the filesystem.

use sober_core::error::AppError;
use sober_core::types::Job;
use sober_core::types::enums::{PluginKind, PluginStatus};
use sober_core::types::input::PluginFilter;
use sober_core::types::repo::PluginRepo;
use tracing::{debug, info};

use crate::executor::{ExecutionResult, JobExecutor};

/// Removes skill plugin entries whose filesystem paths no longer exist.
///
/// Queries all skill plugins (any status), checks the `path` field in their
/// config JSON, and deletes entries where the file is missing.  This handles
/// skills that were removed from `~/.sober/skills/` or `.sober/skills/`
/// without going through the plugins API.
pub struct SkillPluginCleanupExecutor<P: PluginRepo> {
    plugin_repo: P,
}

impl<P: PluginRepo> SkillPluginCleanupExecutor<P> {
    /// Create a new skill plugin cleanup executor.
    pub fn new(plugin_repo: P) -> Self {
        Self { plugin_repo }
    }
}

#[tonic::async_trait]
impl<P: PluginRepo + 'static> JobExecutor for SkillPluginCleanupExecutor<P> {
    async fn execute(&self, _job: &Job) -> Result<ExecutionResult, AppError> {
        let filter = PluginFilter {
            kind: Some(PluginKind::Skill),
            ..Default::default()
        };

        let skill_plugins = self.plugin_repo.list(filter).await?;

        let mut removed = 0u64;
        for plugin in &skill_plugins {
            let path = plugin.config.get("path").and_then(|v| v.as_str());

            let should_remove = match path {
                Some(p) => !std::path::Path::new(p).exists(),
                // No path recorded — mark as failed so it shows up in the UI.
                None => {
                    if plugin.status != PluginStatus::Failed {
                        let _ = self
                            .plugin_repo
                            .update_status(plugin.id, PluginStatus::Failed)
                            .await;
                    }
                    false
                }
            };

            if should_remove {
                debug!(
                    plugin_id = %plugin.id,
                    plugin_name = %plugin.name,
                    path = ?path,
                    "skill file missing, removing plugin entry"
                );
                self.plugin_repo.delete(plugin.id).await?;
                removed += 1;
            }
        }

        info!(
            removed,
            total = skill_plugins.len(),
            "skill plugin cleanup complete"
        );

        Ok(ExecutionResult {
            summary: format!(
                "checked {} skill plugins, removed {removed} stale entries",
                skill_plugins.len()
            ),
            artifact_ref: None,
        })
    }
}
