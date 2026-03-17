//! Audit trail helper functions for the agent.
//!
//! Thin wrappers around [`AuditLogRepo::create`] for common agent operations.
//! Call sites pass structured `serde_json::Value` details; this module handles
//! constructing the [`CreateAuditLog`] input and forwarding to the repo.

use sober_core::error::AppError;
use sober_core::types::{AuditLogRepo, CreateAuditLog, SecretId, UserId, WorkspaceId};

/// Log a shell execution to the audit trail.
pub async fn log_shell_exec<A: AuditLogRepo>(
    audit: &A,
    actor_id: UserId,
    workspace_id: Option<WorkspaceId>,
    details: serde_json::Value,
) -> Result<(), AppError> {
    audit
        .create(CreateAuditLog {
            actor_id: Some(actor_id),
            action: "shell_exec".to_string(),
            target_type: Some("workspace".to_string()),
            target_id: workspace_id.map(|w| *w.as_uuid()),
            details: Some(details),
            ip_address: None,
        })
        .await?;
    Ok(())
}

/// Log a secret access operation.
pub async fn log_secret_access<A: AuditLogRepo>(
    audit: &A,
    actor_id: UserId,
    action: &str,
    secret_id: Option<SecretId>,
    details: serde_json::Value,
) -> Result<(), AppError> {
    audit
        .create(CreateAuditLog {
            actor_id: Some(actor_id),
            action: action.to_string(),
            target_type: Some("secret".to_string()),
            target_id: secret_id.map(|s| *s.as_uuid()),
            details: Some(details),
            ip_address: None,
        })
        .await?;
    Ok(())
}

/// Log a confirmation decision.
pub async fn log_confirmation<A: AuditLogRepo>(
    audit: &A,
    actor_id: UserId,
    approved: bool,
    details: serde_json::Value,
) -> Result<(), AppError> {
    let action = if approved {
        "confirm_approve"
    } else {
        "confirm_deny"
    };
    audit
        .create(CreateAuditLog {
            actor_id: Some(actor_id),
            action: action.to_string(),
            target_type: None,
            target_id: None,
            details: Some(details),
            ip_address: None,
        })
        .await?;
    Ok(())
}
