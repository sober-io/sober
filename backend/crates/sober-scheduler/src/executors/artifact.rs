//! Artifact executor — resolves a blob, runs it in a sandbox, and stores the
//! output as a new artifact.

use std::collections::HashMap;
use std::sync::Arc;

use sober_core::error::AppError;
use sober_core::types::Job;
use sober_core::types::input::CreateSandboxExecutionLog;
use sober_core::types::repo::SandboxExecutionLogRepo;
use sober_db::PgSandboxExecutionLogRepo;
use sober_sandbox::{BwrapSandbox, SandboxPolicy};
use sober_workspace::BlobStore;
use tracing::{info, warn};

use crate::executor::{ExecutionResult, JobExecutor};

/// Executes artifact jobs by resolving a blob, running it in a sandbox, and
/// storing stdout as a result artifact.
pub struct ArtifactExecutor {
    blob_store: Arc<BlobStore>,
    sandbox_policy: SandboxPolicy,
    sandbox_log_repo: Option<Arc<PgSandboxExecutionLogRepo>>,
}

impl ArtifactExecutor {
    /// Create a new artifact executor.
    pub fn new(
        blob_store: Arc<BlobStore>,
        sandbox_policy: SandboxPolicy,
        sandbox_log_repo: Option<Arc<PgSandboxExecutionLogRepo>>,
    ) -> Self {
        Self {
            blob_store,
            sandbox_policy,
            sandbox_log_repo,
        }
    }
}

#[tonic::async_trait]
impl JobExecutor for ArtifactExecutor {
    async fn execute(&self, job: &Job) -> Result<ExecutionResult, AppError> {
        // Extract fields from the job's JSON payload.
        let blob_ref = job.payload["blob_ref"]
            .as_str()
            .ok_or_else(|| AppError::Validation("artifact job missing blob_ref".into()))?;

        let artifact_type = job.payload["artifact_type"].as_str().unwrap_or("script");

        // Verify the blob exists.
        if !self.blob_store.exists(blob_ref).await {
            return Err(AppError::NotFound(format!("blob not found: {blob_ref}")));
        }

        let blob_path = self.blob_store.blob_path(blob_ref);

        // Build command based on artifact type.
        let command: Vec<String> = match artifact_type {
            "wasm" => vec![
                "wasmtime".into(),
                "run".into(),
                blob_path.to_string_lossy().into_owned(),
            ],
            _ => vec!["/bin/sh".into(), blob_path.to_string_lossy().into_owned()],
        };

        // Run in sandbox.
        let sandbox = BwrapSandbox::new(self.sandbox_policy.clone());
        let (result, audit_entry) = sandbox
            .execute(&command, &HashMap::new())
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        // Persist sandbox audit entry.
        if let Some(repo) = &self.sandbox_log_repo {
            let repo = Arc::clone(repo);
            let entry = CreateSandboxExecutionLog {
                execution_id: audit_entry.execution_id,
                workspace_id: None,
                user_id: None,
                policy_name: audit_entry.policy.name.clone(),
                command: audit_entry.command.clone(),
                trigger: format!("{:?}", audit_entry.trigger),
                duration_ms: audit_entry.duration_ms as i64,
                exit_code: audit_entry.exit_code,
                denied_network_requests: audit_entry.denied_network_requests.clone(),
                outcome: format!("{:?}", audit_entry.outcome),
            };
            tokio::spawn(async move {
                if let Err(e) = repo.create(entry).await {
                    warn!(error = %e, "failed to persist sandbox audit log for artifact");
                }
            });
        }

        if result.exit_code != 0 {
            warn!(
                blob_ref,
                exit_code = result.exit_code,
                stderr = %result.stderr,
                "artifact execution failed"
            );
            return Err(AppError::Internal(
                format!(
                    "artifact exited with code {}: {}",
                    result.exit_code, result.stderr
                )
                .into(),
            ));
        }

        // Store stdout as a result artifact.
        let artifact_key = if !result.stdout.is_empty() {
            let key = self
                .blob_store
                .store(result.stdout.as_bytes())
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
            Some(key)
        } else {
            None
        };

        info!(
            blob_ref,
            artifact_type,
            duration_ms = result.duration_ms,
            output_key = ?artifact_key,
            "artifact execution complete"
        );

        Ok(ExecutionResult {
            summary: format!(
                "executed {artifact_type} artifact {blob_ref} in {}ms",
                result.duration_ms
            ),
            artifact_ref: artifact_key,
        })
    }
}
