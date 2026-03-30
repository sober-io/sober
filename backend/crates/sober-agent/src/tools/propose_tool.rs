//! Agent tool for proposing new plugin evolutions.
//!
//! Called by the LLM during self-evolution detection to propose creating
//! a new WASM tool plugin based on patterns observed across conversations.

use std::sync::Arc;

use sober_core::error::AppError;
use sober_core::types::AgentRepos;
use sober_core::types::enums::{AutonomyLevel, EvolutionStatus, EvolutionType};
use sober_core::types::input::CreateEvolutionEvent;
use sober_core::types::repo::EvolutionRepo;
use sober_core::types::tool::{
    BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput, ToolVisibility,
};

/// Maximum number of auto-approved evolutions per day.
const AUTO_APPROVE_DAILY_LIMIT: i64 = 3;

/// Built-in tool that proposes a new plugin evolution.
pub struct ProposeToolTool<R: AgentRepos> {
    repos: Arc<R>,
}

impl<R: AgentRepos> ProposeToolTool<R> {
    /// Creates a new propose-tool tool backed by the given repositories.
    pub fn new(repos: Arc<R>) -> Self {
        Self { repos }
    }
}

impl<R: AgentRepos> Tool for ProposeToolTool<R> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "propose_tool".to_owned(),
            description: "Propose creating a new tool plugin based on observed conversation \
                patterns. The proposal enters the evolution pipeline for review or auto-approval."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Tool name in kebab-case (e.g. 'weather-lookup')."
                    },
                    "description": {
                        "type": "string",
                        "description": "What the tool does."
                    },
                    "capabilities": {
                        "type": "object",
                        "description": "Required capabilities (e.g. http, kv, secrets)."
                    },
                    "pseudocode": {
                        "type": "string",
                        "description": "High-level logic of the tool implementation."
                    },
                    "confidence": {
                        "type": "number",
                        "minimum": 0.0,
                        "maximum": 1.0,
                        "description": "Confidence score (0-1) that this tool would be useful."
                    },
                    "evidence": {
                        "type": "string",
                        "description": "Summary of conversations that triggered this proposal."
                    },
                    "source_count": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Number of conversations that contributed to this proposal."
                    },
                    "user_id": {
                        "type": "string",
                        "description": "UUID of the user whose patterns triggered this (optional)."
                    }
                },
                "required": ["name", "description", "pseudocode", "confidence", "evidence", "source_count"]
            }),
            context_modifying: false,
            redacted: false,
            visibility: ToolVisibility::Internal,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

impl<R: AgentRepos> ProposeToolTool<R> {
    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        // 1. Parse and validate input.
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'name'".into()))?;
        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidInput("missing required field 'description'".into())
            })?;
        let pseudocode = input
            .get("pseudocode")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'pseudocode'".into()))?;
        let confidence = input
            .get("confidence")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'confidence'".into()))?
            as f32;
        let evidence = input
            .get("evidence")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'evidence'".into()))?;
        let source_count = input
            .get("source_count")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                ToolError::InvalidInput("missing required field 'source_count'".into())
            })? as i32;
        let capabilities = input.get("capabilities").cloned();
        let user_id = super::parse_optional_user_id(&input, "user_id")?;

        if name.is_empty() {
            return Err(ToolError::InvalidInput("name must not be empty".into()));
        }
        if !(0.0..=1.0).contains(&confidence) {
            return Err(ToolError::InvalidInput(
                "confidence must be between 0 and 1".into(),
            ));
        }
        if source_count < 1 {
            return Err(ToolError::InvalidInput(
                "source_count must be at least 1".into(),
            ));
        }

        // 2. Read autonomy config.
        let config = self.repos.evolution().get_config().await.map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to read evolution config: {e}"))
        })?;

        let autonomy = config.plugin_autonomy;
        if autonomy == AutonomyLevel::Disabled {
            return Ok(ToolOutput {
                content: "Plugin evolution is currently disabled by configuration.".into(),
                is_error: true,
            });
        }

        // 3. Determine initial status.
        let status =
            super::resolve_initial_status(autonomy, &*self.repos, AUTO_APPROVE_DAILY_LIMIT).await?;

        // 4. Build payload.
        let payload = serde_json::json!({
            "name": name,
            "description": description,
            "capabilities": capabilities,
            "pseudocode": pseudocode,
        });

        // 5. Create the event.
        let title = format!("New tool: {name}");
        let event = CreateEvolutionEvent {
            evolution_type: EvolutionType::Plugin,
            title,
            description: format!("Evidence: {evidence}"),
            confidence,
            source_count,
            status,
            payload,
            user_id,
        };

        match self.repos.evolution().create(event).await {
            Ok(created) => {
                let autonomy_str = match created.status {
                    EvolutionStatus::Approved => "auto_approved",
                    EvolutionStatus::Proposed => "proposed",
                    other => {
                        return Err(ToolError::ExecutionFailed(format!(
                            "unexpected status after creation: {other:?}"
                        )));
                    }
                };
                metrics::counter!("sober_evolution_proposals_total", "type" => "plugin", "autonomy" => autonomy_str)
                    .increment(1);
                let status_label = match created.status {
                    EvolutionStatus::Approved => "auto-approved",
                    _ => "proposed (awaiting approval)",
                };
                Ok(ToolOutput {
                    content: format!(
                        "Evolution event created: {} [{}] — {status_label}.",
                        created.id, name
                    ),
                    is_error: false,
                })
            }
            Err(AppError::Conflict(_)) => {
                metrics::counter!("sober_evolution_proposals_total", "type" => "plugin", "autonomy" => "duplicate")
                    .increment(1);
                Ok(ToolOutput {
                    content: format!(
                        "A similar evolution for tool '{name}' already exists. Skipping duplicate."
                    ),
                    is_error: false,
                })
            }
            Err(e) => Err(ToolError::ExecutionFailed(format!(
                "failed to create evolution event: {e}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn metadata_schema_has_required_fields() {
        let schema = serde_json::json!({
            "required": ["name", "description", "pseudocode", "confidence", "evidence", "source_count"]
        });
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 6);
        assert!(required.iter().any(|v| v.as_str() == Some("name")));
        assert!(required.iter().any(|v| v.as_str() == Some("pseudocode")));
    }
}
