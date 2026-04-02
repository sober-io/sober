use serde::Serialize;
use sober_core::error::AppError;
use sober_core::types::{
    AutonomyLevel, EvolutionConfigRow, EvolutionEvent, EvolutionEventId, EvolutionRepo,
    EvolutionStatus, EvolutionType, UserId,
};
use sober_db::PgEvolutionRepo;
use sqlx::PgPool;
use tracing::instrument;

use crate::proto;
use crate::state::AgentClient;

/// Combined response for evolution config.
#[derive(Serialize)]
pub struct EvolutionConfigResponse {
    pub interval: String,
    pub plugin_autonomy: AutonomyLevel,
    pub skill_autonomy: AutonomyLevel,
    pub instruction_autonomy: AutonomyLevel,
    pub automation_autonomy: AutonomyLevel,
}

/// Typed input for config updates.
pub struct UpdateConfigInput {
    pub plugin_autonomy: Option<AutonomyLevel>,
    pub skill_autonomy: Option<AutonomyLevel>,
    pub instruction_autonomy: Option<AutonomyLevel>,
    pub automation_autonomy: Option<AutonomyLevel>,
}

pub struct EvolutionService {
    db: PgPool,
    agent_client: AgentClient,
    interval: String,
}

impl EvolutionService {
    pub fn new(
        db: PgPool,
        agent_client: AgentClient,
        config: sober_core::config::AppConfig,
    ) -> Self {
        Self {
            db,
            agent_client,
            interval: config.evolution.interval.clone(),
        }
    }

    /// List evolution events with optional filters.
    #[instrument(level = "debug", skip(self))]
    pub async fn list_events(
        &self,
        evolution_type: Option<EvolutionType>,
        status: Option<EvolutionStatus>,
    ) -> Result<Vec<EvolutionEvent>, AppError> {
        let repo = PgEvolutionRepo::new(self.db.clone());
        repo.list(evolution_type, status).await
    }

    /// Get a single evolution event.
    #[instrument(level = "debug", skip(self), fields(evolution.id = %id))]
    pub async fn get_event(&self, id: EvolutionEventId) -> Result<EvolutionEvent, AppError> {
        let repo = PgEvolutionRepo::new(self.db.clone());
        repo.get_by_id(id).await
    }

    /// Update an evolution event's status with state machine validation.
    #[instrument(skip(self), fields(evolution.id = %id))]
    pub async fn update_event(
        &self,
        id: EvolutionEventId,
        target_status: EvolutionStatus,
        admin_user_id: UserId,
    ) -> Result<EvolutionEvent, AppError> {
        let repo = PgEvolutionRepo::new(self.db.clone());
        let current = repo.get_by_id(id).await?;

        validate_status_transition(&current.status, &target_status)?;

        repo.update_status(id, target_status, Some(admin_user_id))
            .await?;

        // Dispatch gRPC calls for statuses that trigger agent-side work.
        match target_status {
            EvolutionStatus::Approved => {
                let mut client = self.agent_client.clone();
                let mut request = tonic::Request::new(proto::ExecuteEvolutionRequest {
                    evolution_event_id: id.to_string(),
                });
                sober_core::inject_trace_context(request.metadata_mut());
                client
                    .execute_evolution(request)
                    .await
                    .map_err(|e| AppError::Internal(e.into()))?;
            }
            EvolutionStatus::Reverted => {
                let mut client = self.agent_client.clone();
                let mut request = tonic::Request::new(proto::RevertEvolutionRequest {
                    evolution_event_id: id.to_string(),
                });
                sober_core::inject_trace_context(request.metadata_mut());
                client
                    .revert_evolution(request)
                    .await
                    .map_err(|e| AppError::Internal(e.into()))?;
            }
            _ => {}
        }

        repo.get_by_id(id).await
    }

    /// Get evolution config.
    #[instrument(level = "debug", skip(self))]
    pub async fn get_config(&self) -> Result<EvolutionConfigResponse, AppError> {
        let repo = PgEvolutionRepo::new(self.db.clone());
        let config = repo.get_config().await?;

        Ok(EvolutionConfigResponse {
            interval: self.interval.clone(),
            plugin_autonomy: config.plugin_autonomy,
            skill_autonomy: config.skill_autonomy,
            instruction_autonomy: config.instruction_autonomy,
            automation_autonomy: config.automation_autonomy,
        })
    }

    /// Update evolution config.
    #[instrument(skip(self, input))]
    pub async fn update_config(
        &self,
        input: UpdateConfigInput,
    ) -> Result<EvolutionConfigResponse, AppError> {
        let repo = PgEvolutionRepo::new(self.db.clone());
        let current = repo.get_config().await?;

        let updated_config = EvolutionConfigRow {
            plugin_autonomy: input.plugin_autonomy.unwrap_or(current.plugin_autonomy),
            skill_autonomy: input.skill_autonomy.unwrap_or(current.skill_autonomy),
            instruction_autonomy: input
                .instruction_autonomy
                .unwrap_or(current.instruction_autonomy),
            automation_autonomy: input
                .automation_autonomy
                .unwrap_or(current.automation_autonomy),
            updated_at: current.updated_at,
        };

        let saved = repo.update_config(updated_config).await?;

        Ok(EvolutionConfigResponse {
            interval: self.interval.clone(),
            plugin_autonomy: saved.plugin_autonomy,
            skill_autonomy: saved.skill_autonomy,
            instruction_autonomy: saved.instruction_autonomy,
            automation_autonomy: saved.automation_autonomy,
        })
    }

    /// Get timeline of evolution events.
    #[instrument(level = "debug", skip(self))]
    pub async fn get_timeline(
        &self,
        limit: i64,
        evolution_type: Option<EvolutionType>,
        status: Option<EvolutionStatus>,
    ) -> Result<Vec<EvolutionEvent>, AppError> {
        let repo = PgEvolutionRepo::new(self.db.clone());
        repo.list_timeline(limit, evolution_type, status).await
    }
}

/// Validates that a status transition is allowed.
fn validate_status_transition(
    from: &EvolutionStatus,
    to: &EvolutionStatus,
) -> Result<(), AppError> {
    match (from, to) {
        (EvolutionStatus::Proposed | EvolutionStatus::Failed, EvolutionStatus::Approved) => Ok(()),
        (EvolutionStatus::Proposed, EvolutionStatus::Rejected) => Ok(()),
        (EvolutionStatus::Active, EvolutionStatus::Reverted) => Ok(()),
        _ => Err(AppError::Validation(format!(
            "cannot transition from {} to {}",
            serde_json::to_string(from)
                .unwrap_or_default()
                .trim_matches('"'),
            serde_json::to_string(to)
                .unwrap_or_default()
                .trim_matches('"'),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_transitions() {
        // Proposed → Approved
        assert!(
            validate_status_transition(&EvolutionStatus::Proposed, &EvolutionStatus::Approved)
                .is_ok()
        );
        // Failed → Approved (retry)
        assert!(
            validate_status_transition(&EvolutionStatus::Failed, &EvolutionStatus::Approved)
                .is_ok()
        );
        // Proposed → Rejected
        assert!(
            validate_status_transition(&EvolutionStatus::Proposed, &EvolutionStatus::Rejected)
                .is_ok()
        );
        // Active → Reverted
        assert!(
            validate_status_transition(&EvolutionStatus::Active, &EvolutionStatus::Reverted)
                .is_ok()
        );
    }

    #[test]
    fn invalid_transitions() {
        // Active → Approved (already past approval)
        assert!(
            validate_status_transition(&EvolutionStatus::Active, &EvolutionStatus::Approved)
                .is_err()
        );
        // Rejected → Approved (must re-propose)
        assert!(
            validate_status_transition(&EvolutionStatus::Rejected, &EvolutionStatus::Approved)
                .is_err()
        );
        // Proposed → Reverted (not yet active)
        assert!(
            validate_status_transition(&EvolutionStatus::Proposed, &EvolutionStatus::Reverted)
                .is_err()
        );
        // Active → Rejected (already active)
        assert!(
            validate_status_transition(&EvolutionStatus::Active, &EvolutionStatus::Rejected)
                .is_err()
        );
        // Reverted → anything
        assert!(
            validate_status_transition(&EvolutionStatus::Reverted, &EvolutionStatus::Approved)
                .is_err()
        );
    }
}
