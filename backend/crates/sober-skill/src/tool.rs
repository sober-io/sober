//! The `activate_skill` tool — loads skill instructions into conversation context.

use std::sync::Arc;

use sober_core::types::tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
use tokio::fs;

use crate::catalog::SkillCatalog;
use crate::frontmatter::parse_skill_frontmatter;
use crate::types::SkillActivationState;

/// Tool that activates a skill by loading its instructions into conversation context.
///
/// When executed, reads the skill's `SKILL.md`, strips frontmatter, wraps
/// the body in `<skill_content>` tags, and marks the skill as activated for
/// the current conversation to prevent duplicate injection.
pub struct ActivateSkillTool {
    catalog: Arc<SkillCatalog>,
    activation_state: Arc<std::sync::Mutex<SkillActivationState>>,
}

impl ActivateSkillTool {
    /// Creates a new `ActivateSkillTool` with the given catalog and activation state.
    pub fn new(
        catalog: Arc<SkillCatalog>,
        activation_state: Arc<std::sync::Mutex<SkillActivationState>>,
    ) -> Self {
        Self {
            catalog,
            activation_state,
        }
    }

    /// Core execution logic (exposed for testing).
    pub(crate) async fn execute_inner(
        &self,
        input: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing 'name' field".into()))?;

        let entry = match self.catalog.get(name) {
            Some(e) => e,
            None => {
                return Ok(ToolOutput {
                    content: format!("Skill not found: {name}"),
                    is_error: true,
                });
            }
        };

        // Check already activated (acquire and release lock before any await)
        {
            let state = self.activation_state.lock().map_err(|_| {
                ToolError::Internal(Box::new(std::io::Error::other(
                    "activation state lock poisoned",
                )))
            })?;
            if state.is_activated(name) {
                return Ok(ToolOutput {
                    content: format!("Skill '{name}' is already active in this conversation."),
                    is_error: false,
                });
            }
        }

        // Read SKILL.md and strip frontmatter
        let content = fs::read_to_string(&entry.path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to read SKILL.md: {e}")))?;

        let (_fm, body) = parse_skill_frontmatter(&content)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to parse SKILL.md: {e}")))?;

        let resources = enumerate_resources(&entry.base_dir).await;

        let mut result = format!("<skill_content name=\"{name}\">\n");

        if let Some(compat) = &entry.frontmatter.compatibility {
            result.push_str(&format!("Compatibility: {compat}\n\n"));
        }

        result.push_str(body);
        result.push_str(&format!(
            "\n\nSkill directory: {}\nRelative paths are relative to the skill directory.",
            entry.base_dir.display()
        ));

        if !resources.is_empty() {
            result.push_str("\n\n<skill_resources>\n");
            for resource in &resources {
                result.push_str(&format!("  <file>{resource}</file>\n"));
            }
            result.push_str("</skill_resources>");
        }

        result.push_str("\n</skill_content>");

        // Mark as activated
        {
            let mut state = self.activation_state.lock().map_err(|_| {
                ToolError::Internal(Box::new(std::io::Error::other(
                    "activation state lock poisoned",
                )))
            })?;
            state.activate(name.to_string());
        }

        Ok(ToolOutput {
            content: result,
            is_error: false,
        })
    }
}

impl Tool for ActivateSkillTool {
    fn metadata(&self) -> ToolMetadata {
        let names = self.catalog.names();
        let enum_values: Vec<serde_json::Value> = names
            .iter()
            .map(|n| serde_json::Value::String(n.to_string()))
            .collect();

        ToolMetadata {
            name: "activate_skill".to_owned(),
            description: "Load specialized skill instructions into the conversation. \
                Call when a task matches an available skill's description."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "enum": enum_values,
                        "description": "The skill to activate"
                    }
                },
                "required": ["name"]
            }),
            context_modifying: false,
            internal: false,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

/// Enumerates resource files from `scripts/`, `references/`, and `assets/`
/// subdirectories within a skill's base directory.
async fn enumerate_resources(base_dir: &std::path::Path) -> Vec<String> {
    let mut resources = Vec::new();

    for subdir in ["scripts", "references", "assets"] {
        let dir = base_dir.join(subdir);
        if !dir.is_dir() {
            continue;
        }
        let Ok(mut entries) = fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.path().is_file()
                && let Some(name) = entry.file_name().to_str()
            {
                resources.push(format!("{subdir}/{name}"));
            }
        }
    }

    resources.sort();
    resources
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::SkillFrontmatter;
    use crate::types::{SkillActivationState, SkillEntry, SkillSource};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn test_catalog() -> Arc<SkillCatalog> {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_md = skill_dir.join("SKILL.md");
        std::fs::write(
            &skill_md,
            "---\nname: test-skill\ndescription: A test skill.\n---\n\n## Instructions\n\nDo the thing.",
        )
        .unwrap();

        let mut skills = HashMap::new();
        skills.insert(
            "test-skill".into(),
            SkillEntry {
                frontmatter: SkillFrontmatter {
                    name: "test-skill".into(),
                    description: "A test skill.".into(),
                    license: None,
                    compatibility: None,
                    metadata: None,
                    allowed_tools: None,
                },
                path: skill_md,
                base_dir: skill_dir,
                source: SkillSource::User,
            },
        );

        std::mem::forget(tmp); // Keep temp files alive

        Arc::new(SkillCatalog::new(skills))
    }

    #[tokio::test]
    async fn activate_returns_skill_content() {
        let catalog = test_catalog();
        let state = Arc::new(Mutex::new(SkillActivationState::default()));
        let tool = ActivateSkillTool::new(Arc::clone(&catalog), Arc::clone(&state));

        let input = serde_json::json!({ "name": "test-skill" });
        let output = tool.execute_inner(input).await.unwrap();

        assert!(!output.is_error);
        assert!(output.content.contains("<skill_content"));
        assert!(output.content.contains("Do the thing."));
        assert!(output.content.contains("</skill_content>"));
    }

    #[tokio::test]
    async fn duplicate_activation_returns_already_active() {
        let catalog = test_catalog();
        let state = Arc::new(Mutex::new(SkillActivationState::default()));
        let tool = ActivateSkillTool::new(Arc::clone(&catalog), Arc::clone(&state));

        let input = serde_json::json!({ "name": "test-skill" });
        tool.execute_inner(input.clone()).await.unwrap();

        let output = tool.execute_inner(input).await.unwrap();
        assert!(output.content.contains("already active"));
    }

    #[tokio::test]
    async fn not_found_returns_error() {
        let catalog = test_catalog();
        let state = Arc::new(Mutex::new(SkillActivationState::default()));
        let tool = ActivateSkillTool::new(Arc::clone(&catalog), Arc::clone(&state));

        let input = serde_json::json!({ "name": "nonexistent" });
        let output = tool.execute_inner(input).await.unwrap();
        assert!(output.is_error);
    }

    #[test]
    fn metadata_has_dynamic_enum() {
        let catalog = test_catalog();
        let state = Arc::new(Mutex::new(SkillActivationState::default()));
        let tool = ActivateSkillTool::new(catalog, state);
        let meta = tool.metadata();

        assert_eq!(meta.name, "activate_skill");
        assert!(!meta.context_modifying);
        assert!(!meta.internal);

        let schema = &meta.input_schema;
        let enum_values = schema["properties"]["name"]["enum"].as_array().unwrap();
        assert!(enum_values.iter().any(|v| v == "test-skill"));
    }
}
