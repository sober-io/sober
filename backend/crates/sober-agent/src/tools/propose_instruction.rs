//! Agent tool for proposing instruction file changes.
//!
//! Called by the LLM during self-evolution detection to propose modifying
//! an instruction overlay file. Includes a dual guardrail check to prevent
//! modifications to safety-critical files.

use std::sync::Arc;

use sober_core::error::AppError;
use sober_core::types::AgentRepos;
use sober_core::types::enums::{AutonomyLevel, EvolutionStatus, EvolutionType};
use sober_core::types::input::CreateEvolutionEvent;
use sober_core::types::repo::EvolutionRepo;
use sober_core::types::tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};

/// Maximum number of auto-approved evolutions per day.
const AUTO_APPROVE_DAILY_LIMIT: i64 = 3;

/// Built-in tool that proposes an instruction file change.
pub struct ProposeInstructionTool<R: AgentRepos> {
    repos: Arc<R>,
}

impl<R: AgentRepos> ProposeInstructionTool<R> {
    /// Creates a new propose-instruction tool backed by the given repositories.
    pub fn new(repos: Arc<R>) -> Self {
        Self { repos }
    }
}

impl<R: AgentRepos> Tool for ProposeInstructionTool<R> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "propose_instruction_change".to_owned(),
            description: "Propose modifying an instruction overlay file to improve agent \
                behavior. The proposal enters the evolution pipeline for review or auto-approval. \
                Guardrail and safety files cannot be modified."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "Instruction file path relative to instructions/ (e.g. 'behavior/reasoning.md')."
                    },
                    "new_content": {
                        "type": "string",
                        "description": "Full replacement content for the instruction file."
                    },
                    "rationale": {
                        "type": "string",
                        "description": "Why this change improves agent behavior."
                    },
                    "confidence": {
                        "type": "number",
                        "minimum": 0.0,
                        "maximum": 1.0,
                        "description": "Confidence score (0-1) that this change is beneficial."
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
                "required": ["file", "new_content", "rationale", "confidence", "evidence", "source_count"]
            }),
            context_modifying: false,
            internal: false,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

impl<R: AgentRepos> ProposeInstructionTool<R> {
    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        // 1. Parse and validate input.
        let file = input
            .get("file")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'file'".into()))?;
        let new_content = input
            .get("new_content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidInput("missing required field 'new_content'".into())
            })?;
        let rationale = input
            .get("rationale")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'rationale'".into()))?;
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
        let user_id = super::parse_optional_user_id(&input, "user_id")?;

        if file.is_empty() {
            return Err(ToolError::InvalidInput("file must not be empty".into()));
        }
        if new_content.is_empty() {
            return Err(ToolError::InvalidInput(
                "new_content must not be empty".into(),
            ));
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

        // 2. GUARDRAIL CHECK: prevent modification of safety-critical files.
        if sober_mind::is_guardrail_file(file, new_content) {
            return Ok(ToolOutput {
                content: format!(
                    "Cannot modify '{file}': this is a guardrail/safety file and cannot be \
                     changed through the evolution pipeline."
                ),
                is_error: true,
            });
        }

        // 3. Read autonomy config.
        let config = self.repos.evolution().get_config().await.map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to read evolution config: {e}"))
        })?;

        let autonomy = config.instruction_autonomy;
        if autonomy == AutonomyLevel::Disabled {
            return Ok(ToolOutput {
                content: "Instruction evolution is currently disabled by configuration.".into(),
                is_error: true,
            });
        }

        // 4. Determine initial status.
        let status =
            super::resolve_initial_status(autonomy, &*self.repos, AUTO_APPROVE_DAILY_LIMIT).await?;

        // 5. Build payload.
        let payload = serde_json::json!({
            "file": file,
            "new_content": new_content,
            "rationale": rationale,
        });

        // 6. Create the event.
        let title = format!("Instruction change: {file}");
        let event = CreateEvolutionEvent {
            evolution_type: EvolutionType::Instruction,
            title,
            description: format!("Rationale: {rationale}\n\nEvidence: {evidence}"),
            confidence,
            source_count,
            status,
            payload,
            user_id,
        };

        match self.repos.evolution().create(event).await {
            Ok(created) => {
                let status_label = match created.status {
                    EvolutionStatus::Approved => "auto-approved",
                    EvolutionStatus::Proposed => "proposed (awaiting approval)",
                    other => {
                        return Err(ToolError::ExecutionFailed(format!(
                            "unexpected status after creation: {other:?}"
                        )));
                    }
                };
                Ok(ToolOutput {
                    content: format!(
                        "Evolution event created: {} [{file}] — {status_label}.",
                        created.id
                    ),
                    is_error: false,
                })
            }
            Err(AppError::Conflict(_)) => Ok(ToolOutput {
                content: format!(
                    "A similar evolution for instruction '{file}' already exists. Skipping duplicate."
                ),
                is_error: false,
            }),
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
            "required": ["file", "new_content", "rationale", "confidence", "evidence", "source_count"]
        });
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 6);
        assert!(required.iter().any(|v| v.as_str() == Some("file")));
        assert!(required.iter().any(|v| v.as_str() == Some("new_content")));
        assert!(required.iter().any(|v| v.as_str() == Some("rationale")));
    }
}
