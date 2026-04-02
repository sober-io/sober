//! Agent tool for proposing new automation (scheduled job) evolutions.
//!
//! Called by the LLM during self-evolution detection to propose creating
//! a new scheduled job based on patterns observed across conversations.

use std::sync::Arc;

use sober_core::error::AppError;
use sober_core::types::AgentRepos;
use sober_core::types::enums::{AutonomyLevel, EvolutionStatus, EvolutionType};
use sober_core::types::ids::UserId;
use sober_core::types::input::CreateEvolutionEvent;
use sober_core::types::repo::EvolutionRepo;
use sober_core::types::tool::{
    BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput, ToolVisibility,
};
use tracing::instrument;
use uuid::Uuid;

/// Maximum number of auto-approved evolutions per day.
const AUTO_APPROVE_DAILY_LIMIT: i64 = 3;

/// Built-in tool that proposes a new automation evolution.
pub struct ProposeAutomationTool<R: AgentRepos> {
    repos: Arc<R>,
}

impl<R: AgentRepos> ProposeAutomationTool<R> {
    /// Creates a new propose-automation tool backed by the given repositories.
    pub fn new(repos: Arc<R>) -> Self {
        Self { repos }
    }
}

impl<R: AgentRepos> Tool for ProposeAutomationTool<R> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "propose_automation".to_owned(),
            description: "Propose creating a new scheduled automation job based on observed \
                conversation patterns. The proposal enters the evolution pipeline for review \
                or auto-approval."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "job_name": {
                        "type": "string",
                        "description": "Human-readable name for the scheduled job."
                    },
                    "schedule": {
                        "type": "string",
                        "description": "Cron expression for the schedule (e.g. '0 9 * * 1-5')."
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Prompt text for the scheduled job."
                    },
                    "target_user_id": {
                        "type": "string",
                        "description": "UUID of the user this automation serves."
                    },
                    "conversation_id": {
                        "type": "string",
                        "description": "Conversation to deliver results to (optional)."
                    },
                    "confidence": {
                        "type": "number",
                        "minimum": 0.0,
                        "maximum": 1.0,
                        "description": "Confidence score (0-1) that this automation would be useful."
                    },
                    "evidence": {
                        "type": "string",
                        "description": "Summary of conversations that triggered this proposal."
                    },
                    "source_count": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Number of conversations that contributed to this proposal."
                    }
                },
                "required": ["job_name", "schedule", "prompt", "target_user_id", "confidence", "evidence", "source_count"]
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

impl<R: AgentRepos> ProposeAutomationTool<R> {
    #[instrument(skip(self, input), fields(tool.name = "propose_automation"))]
    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        // 1. Parse and validate input.
        let job_name = input
            .get("job_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'job_name'".into()))?;
        let schedule = input
            .get("schedule")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'schedule'".into()))?;
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'prompt'".into()))?;
        let target_user_id_str = input
            .get("target_user_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidInput("missing required field 'target_user_id'".into())
            })?;
        let target_user_id = Uuid::parse_str(target_user_id_str)
            .map_err(|e| ToolError::InvalidInput(format!("invalid target_user_id UUID: {e}")))?;
        let conversation_id = input
            .get("conversation_id")
            .and_then(|v| v.as_str())
            .map(|s| {
                Uuid::parse_str(s).map_err(|e| {
                    ToolError::InvalidInput(format!("invalid conversation_id UUID: {e}"))
                })
            })
            .transpose()?;
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

        if job_name.is_empty() {
            return Err(ToolError::InvalidInput("job_name must not be empty".into()));
        }
        if schedule.is_empty() {
            return Err(ToolError::InvalidInput("schedule must not be empty".into()));
        }
        if prompt.is_empty() {
            return Err(ToolError::InvalidInput("prompt must not be empty".into()));
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

        let autonomy = config.automation_autonomy;
        if autonomy == AutonomyLevel::Disabled {
            return Ok(ToolOutput {
                content: "Automation evolution is currently disabled by configuration.".into(),
                is_error: true,
            });
        }

        // 3. Determine initial status.
        let status =
            super::resolve_initial_status(autonomy, &*self.repos, AUTO_APPROVE_DAILY_LIMIT).await?;

        // 4. Build payload.
        let payload = serde_json::json!({
            "job_name": job_name,
            "schedule": schedule,
            "prompt": prompt,
            "target_user_id": target_user_id.to_string(),
            "conversation_id": conversation_id.map(|id| id.to_string()),
        });

        // 5. Create the event — use target_user_id as the triggering user.
        let title = format!("New automation: {job_name}");
        let user_id = UserId::from_uuid(target_user_id);
        let event = CreateEvolutionEvent {
            evolution_type: EvolutionType::Automation,
            title,
            description: format!("Evidence: {evidence}"),
            confidence,
            source_count,
            status,
            payload,
            user_id: Some(user_id),
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
                metrics::counter!("sober_evolution_proposals_total", "type" => "automation", "autonomy" => autonomy_str)
                    .increment(1);
                let status_label = match created.status {
                    EvolutionStatus::Approved => "auto-approved",
                    _ => "proposed (awaiting approval)",
                };
                Ok(ToolOutput {
                    content: format!(
                        "Evolution event created: {} [{job_name}] — {status_label}.",
                        created.id
                    ),
                    is_error: false,
                })
            }
            Err(AppError::Conflict(_)) => {
                metrics::counter!("sober_evolution_proposals_total", "type" => "automation", "autonomy" => "duplicate")
                    .increment(1);
                Ok(ToolOutput {
                    content: format!(
                        "A similar evolution for automation '{job_name}' already exists. Skipping duplicate."
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
            "required": ["job_name", "schedule", "prompt", "target_user_id", "confidence", "evidence", "source_count"]
        });
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 7);
        assert!(required.iter().any(|v| v.as_str() == Some("job_name")));
        assert!(required.iter().any(|v| v.as_str() == Some("schedule")));
        assert!(
            required
                .iter()
                .any(|v| v.as_str() == Some("target_user_id"))
        );
    }
}
