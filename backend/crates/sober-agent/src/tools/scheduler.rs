//! Agent tools for managing scheduled jobs via the scheduler gRPC service.
//!
//! Implements the [`Tool`] trait so the LLM can manage scheduled jobs
//! conversationally. Dispatches on an `action` field to the appropriate
//! scheduler gRPC call.

use sober_core::types::tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
use uuid::Uuid;

use crate::SharedSchedulerClient;
use crate::grpc::scheduler_proto;

/// Job action for authorization checks.
#[derive(Debug, Clone, Copy)]
pub enum JobAction {
    /// Viewing job details or listing jobs.
    View,
    /// Creating a new job.
    Create,
    /// Modifying an existing job (pause/resume).
    Modify,
    /// Cancelling a job.
    Cancel,
}

/// Agent tools for managing scheduled jobs via the scheduler gRPC service.
pub struct SchedulerTools {
    scheduler_client: SharedSchedulerClient,
}

impl SchedulerTools {
    /// Creates a new `SchedulerTools` with the given scheduler client handle.
    pub fn new(scheduler_client: SharedSchedulerClient) -> Self {
        Self { scheduler_client }
    }

    /// Checks authorization for a job action based on owner type and caller.
    ///
    /// When `caller_user_id` is nil (no user context), authorization is skipped —
    /// the agent is acting autonomously or the caller identity hasn't been
    /// injected yet.
    fn authorize(
        &self,
        caller_user_id: Uuid,
        job: &scheduler_proto::Job,
        action: JobAction,
        is_admin: bool,
    ) -> Result<(), String> {
        if caller_user_id.is_nil() {
            return Ok(());
        }
        match (job.owner_type.as_str(), action) {
            ("user", _) => {
                if job.created_by != caller_user_id.to_string() {
                    return Err("Forbidden: not the job owner".into());
                }
            }
            ("group", JobAction::View | JobAction::Create) => {
                // TODO: check group membership via auth service
            }
            ("group", _) => {
                if job.created_by != caller_user_id.to_string() {
                    // TODO: check group admin status
                    return Err("Forbidden: not the job creator or group admin".into());
                }
            }
            ("system", _) if is_admin => {
                // Admins can view system jobs but not modify them via conversation.
                if !matches!(action, JobAction::View) {
                    return Err(
                        "System jobs can only be viewed, not modified via conversation".into(),
                    );
                }
            }
            ("system", _) => {
                return Err("System jobs cannot be accessed via conversation".into());
            }
            _ => return Err("Unknown owner type".into()),
        }
        Ok(())
    }

    /// Creates a new scheduled job.
    ///
    /// Checks for an existing active job with the same name and owner before
    /// creating. Returns the existing job info if a duplicate is found.
    pub async fn create_job(
        &self,
        name: &str,
        schedule: &str,
        payload: sober_core::types::JobPayload,
        caller_user_id: Uuid,
        workspace_id: Option<Uuid>,
        conversation_id: Option<Uuid>,
    ) -> Result<String, String> {
        // Check for existing active job with the same name + owner.
        let has_user = !caller_user_id.is_nil();
        {
            // Check for existing active/running job with the same name.
            let mut client = {
                let guard = self.scheduler_client.read().await;
                guard.as_ref().ok_or("Scheduler not connected")?.clone()
            };
            let req = scheduler_proto::ListJobsRequest {
                owner_type: None,
                owner_id: if has_user {
                    Some(caller_user_id.to_string())
                } else {
                    None
                },
                statuses: vec!["active".into(), "running".into()],
                workspace_id: String::new(),
                name_filter: name.into(),
            };
            if let Ok(response) = client.list_jobs(req).await
                && let Some(job) = response
                    .into_inner()
                    .jobs
                    .into_iter()
                    .find(|j| j.name == name)
            {
                return Ok(format!(
                    "Job '{}' already exists ({}, status: {}). Next run: {}",
                    job.name, job.id, job.status, job.next_run_at
                ));
            }
        }

        let payload_bytes = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;

        // TODO: detect group workspace and set owner_type accordingly
        let owner_type = "user";

        let req = scheduler_proto::CreateJobRequest {
            name: name.into(),
            owner_type: owner_type.into(),
            owner_id: if has_user {
                Some(caller_user_id.to_string())
            } else {
                None
            },
            schedule: schedule.into(),
            payload: payload_bytes,
            workspace_id: workspace_id.map(|id| id.to_string()).unwrap_or_default(),
            created_by: if has_user {
                caller_user_id.to_string()
            } else {
                String::new()
            },
            conversation_id: conversation_id.map(|id| id.to_string()).unwrap_or_default(),
        };

        let mut client = {
            let guard = self.scheduler_client.read().await;
            guard.as_ref().ok_or("Scheduler not connected")?.clone()
        };
        let response = client.create_job(req).await.map_err(|e| e.to_string())?;
        let job = response.into_inner();

        Ok(format!(
            "Created job '{}' ({}). Next run: {}",
            job.name, job.id, job.next_run_at
        ))
    }

    /// Lists jobs with optional filters.
    ///
    /// System jobs are only visible to admin users.
    pub async fn list_jobs(
        &self,
        owner_type: Option<&str>,
        owner_id: Option<Uuid>,
        statuses: Vec<String>,
        workspace_id: Option<Uuid>,
        name_filter: Option<&str>,
        is_admin: bool,
    ) -> Result<String, String> {
        let req = scheduler_proto::ListJobsRequest {
            owner_type: owner_type.map(|s| s.into()),
            owner_id: owner_id.map(|id| id.to_string()),
            statuses,
            workspace_id: workspace_id.map(|id| id.to_string()).unwrap_or_default(),
            name_filter: name_filter.unwrap_or_default().into(),
        };

        let mut client = {
            let guard = self.scheduler_client.read().await;
            guard.as_ref().ok_or("Scheduler not connected")?.clone()
        };
        let response = client.list_jobs(req).await.map_err(|e| e.to_string())?;
        let jobs: Vec<_> = response
            .into_inner()
            .jobs
            .into_iter()
            .filter(|j| is_admin || j.owner_type != "system")
            .collect();

        if jobs.is_empty() {
            return Ok("No jobs found.".into());
        }

        let lines: Vec<String> = jobs
            .iter()
            .map(|j| {
                format!(
                    "- {} ({}) [{}] next: {}",
                    j.name, j.id, j.status, j.next_run_at
                )
            })
            .collect();

        Ok(lines.join("\n"))
    }

    /// Gets a single job by ID.
    pub async fn get_job(
        &self,
        job_id: &str,
        caller_user_id: Uuid,
        is_admin: bool,
    ) -> Result<String, String> {
        let req = scheduler_proto::GetJobRequest {
            job_id: job_id.into(),
        };

        let mut client = {
            let guard = self.scheduler_client.read().await;
            guard.as_ref().ok_or("Scheduler not connected")?.clone()
        };
        let response = client.get_job(req).await.map_err(|e| e.to_string())?;
        let job = response.into_inner();

        self.authorize(caller_user_id, &job, JobAction::View, is_admin)?;

        Ok(format!(
            "Job '{}' ({})\n  Status: {}\n  Schedule: {}\n  Next run: {}\n  Last run: {}",
            job.name,
            job.id,
            job.status,
            String::new(), // schedule not in proto response, use status
            job.next_run_at,
            job.last_run_at.as_deref().unwrap_or("never"),
        ))
    }

    /// Cancels a job by ID.
    pub async fn cancel_job(
        &self,
        job_id: &str,
        caller_user_id: Uuid,
        is_admin: bool,
    ) -> Result<String, String> {
        // Fetch the job first for authorization
        let get_req = scheduler_proto::GetJobRequest {
            job_id: job_id.into(),
        };
        let mut client = {
            let guard = self.scheduler_client.read().await;
            guard.as_ref().ok_or("Scheduler not connected")?.clone()
        };
        let job = client
            .get_job(get_req)
            .await
            .map_err(|e| e.to_string())?
            .into_inner();

        self.authorize(caller_user_id, &job, JobAction::Cancel, is_admin)?;

        let cancel_req = scheduler_proto::CancelJobRequest {
            job_id: job_id.into(),
        };
        client
            .cancel_job(cancel_req)
            .await
            .map_err(|e| e.to_string())?;

        Ok(format!("Cancelled job '{}'", job.name))
    }

    /// Pauses a job by ID.
    pub async fn pause_job(
        &self,
        job_id: &str,
        caller_user_id: Uuid,
        is_admin: bool,
    ) -> Result<String, String> {
        let get_req = scheduler_proto::GetJobRequest {
            job_id: job_id.into(),
        };
        let mut client = {
            let guard = self.scheduler_client.read().await;
            guard.as_ref().ok_or("Scheduler not connected")?.clone()
        };
        let job = client
            .get_job(get_req)
            .await
            .map_err(|e| e.to_string())?
            .into_inner();

        self.authorize(caller_user_id, &job, JobAction::Modify, is_admin)?;

        let req = scheduler_proto::PauseJobRequest {
            job_id: job_id.into(),
        };
        let response = client.pause_job(req).await.map_err(|e| e.to_string())?;
        let updated = response.into_inner().job.ok_or("No job in response")?;

        Ok(format!(
            "Paused job '{}' ({})",
            updated.name, updated.status
        ))
    }

    /// Resumes a paused job by ID.
    pub async fn resume_job(
        &self,
        job_id: &str,
        caller_user_id: Uuid,
        is_admin: bool,
    ) -> Result<String, String> {
        let get_req = scheduler_proto::GetJobRequest {
            job_id: job_id.into(),
        };
        let mut client = {
            let guard = self.scheduler_client.read().await;
            guard.as_ref().ok_or("Scheduler not connected")?.clone()
        };
        let job = client
            .get_job(get_req)
            .await
            .map_err(|e| e.to_string())?
            .into_inner();

        self.authorize(caller_user_id, &job, JobAction::Modify, is_admin)?;

        let req = scheduler_proto::ResumeJobRequest {
            job_id: job_id.into(),
        };
        let response = client.resume_job(req).await.map_err(|e| e.to_string())?;
        let updated = response.into_inner().job.ok_or("No job in response")?;

        Ok(format!(
            "Resumed job '{}' ({}). Next run: {}",
            updated.name, updated.status, updated.next_run_at
        ))
    }

    /// Lists recent runs for a job.
    pub async fn get_job_runs(
        &self,
        job_id: &str,
        caller_user_id: Uuid,
        is_admin: bool,
        limit: Option<u32>,
    ) -> Result<String, String> {
        // Fetch job for authorization
        let get_req = scheduler_proto::GetJobRequest {
            job_id: job_id.into(),
        };
        let mut client = {
            let guard = self.scheduler_client.read().await;
            guard.as_ref().ok_or("Scheduler not connected")?.clone()
        };
        let job = client
            .get_job(get_req)
            .await
            .map_err(|e| e.to_string())?
            .into_inner();

        self.authorize(caller_user_id, &job, JobAction::View, is_admin)?;

        let req = scheduler_proto::ListJobRunsRequest {
            job_id: job_id.into(),
            limit,
        };
        let response = client.list_job_runs(req).await.map_err(|e| e.to_string())?;
        let runs = response.into_inner().runs;

        if runs.is_empty() {
            return Ok("No runs found.".into());
        }

        let lines: Vec<String> = runs
            .iter()
            .map(|r| {
                let finished = r.finished_at.as_deref().unwrap_or("running");
                let error_info = r
                    .error
                    .as_deref()
                    .map(|e| format!("\n  error: {e}"))
                    .unwrap_or_default();
                let result_info = if r.result.is_empty() {
                    String::new()
                } else {
                    let text = String::from_utf8_lossy(&r.result);
                    format!("\n  result: {text}")
                };
                format!(
                    "- {} [{}] started: {} finished: {}{}{}",
                    r.id, r.status, r.started_at, finished, result_info, error_info
                )
            })
            .collect();

        Ok(lines.join("\n"))
    }

    /// Inner dispatch for the [`Tool`] implementation.
    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'action'".into()))?;

        // owner_id and is_admin are injected by the agent loop.
        let owner_id_raw = input.get("owner_id").and_then(|v| v.as_str());
        let caller_user_id = owner_id_raw.and_then(|s| Uuid::parse_str(s).ok());
        let is_admin = input
            .get("is_admin")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let result = match action {
            "list" => {
                let owner_type = input.get("owner_type").and_then(|v| v.as_str());
                let statuses: Vec<String> =
                    if let Some(s) = input.get("status").and_then(|v| v.as_str()) {
                        vec![s.to_owned()]
                    } else if let Some(arr) = input.get("statuses").and_then(|v| v.as_array()) {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                            .collect()
                    } else {
                        vec![]
                    };
                let name_filter = input.get("name_filter").and_then(|v| v.as_str());
                // Admins can list all jobs; non-admins are scoped to their own.
                let owner_id = if is_admin { None } else { caller_user_id };
                let workspace_id = input
                    .get("workspace_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok());
                self.list_jobs(
                    owner_type,
                    owner_id,
                    statuses,
                    workspace_id,
                    name_filter,
                    is_admin,
                )
                .await
            }
            "get" => {
                let job_id = input
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidInput("missing 'job_id' for get action".into())
                    })?;
                let uid = caller_user_id.unwrap_or_else(Uuid::nil);
                self.get_job(job_id, uid, is_admin).await
            }
            "create" => {
                let name = input.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                    ToolError::InvalidInput("missing 'name' for create action".into())
                })?;
                let schedule = input
                    .get("schedule")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidInput("missing 'schedule' for create action".into())
                    })?;
                let payload = parse_job_payload(&input)?;
                let uid = caller_user_id.unwrap_or_else(Uuid::nil);
                let workspace_id = input
                    .get("workspace_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok());
                let conversation_id = input
                    .get("conversation_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok());
                self.create_job(name, schedule, payload, uid, workspace_id, conversation_id)
                    .await
            }
            "cancel" => {
                let job_id = input
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidInput("missing 'job_id' for cancel action".into())
                    })?;
                let uid = caller_user_id.unwrap_or_else(Uuid::nil);
                self.cancel_job(job_id, uid, is_admin).await
            }
            "pause" => {
                let job_id = input
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidInput("missing 'job_id' for pause action".into())
                    })?;
                let uid = caller_user_id.unwrap_or_else(Uuid::nil);
                self.pause_job(job_id, uid, is_admin).await
            }
            "resume" => {
                let job_id = input
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidInput("missing 'job_id' for resume action".into())
                    })?;
                let uid = caller_user_id.unwrap_or_else(Uuid::nil);
                self.resume_job(job_id, uid, is_admin).await
            }
            "runs" => {
                let job_id = input
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidInput("missing 'job_id' for runs action".into())
                    })?;
                let uid = caller_user_id.unwrap_or_else(Uuid::nil);
                let limit = input
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
                self.get_job_runs(job_id, uid, is_admin, limit).await
            }
            _ => {
                return Err(ToolError::InvalidInput(format!(
                    "unknown action '{action}'. Valid actions: list, get, create, cancel, pause, resume, runs"
                )));
            }
        };

        match result {
            Ok(content) => Ok(ToolOutput {
                content,
                is_error: false,
            }),
            Err(msg) => Ok(ToolOutput {
                content: msg,
                is_error: true,
            }),
        }
    }
}

/// Parse a flat JSON payload from the LLM into a typed `JobPayload`.
///
/// The LLM sends `{"payload_type": "prompt", "text": "..."}` style flat JSON.
/// We translate that into the Rust enum which serde serialises as externally tagged.
fn parse_job_payload(
    input: &serde_json::Value,
) -> Result<sober_core::types::JobPayload, ToolError> {
    use sober_core::types::JobPayload;

    let payload_type = input
        .get("payload_type")
        .and_then(|v| v.as_str())
        .or_else(|| {
            input
                .get("payload")
                .and_then(|p| p.get("type"))
                .and_then(|v| v.as_str())
        })
        .ok_or_else(|| {
            ToolError::InvalidInput(
                "missing 'payload_type' (or payload.type). Use 'prompt', 'artifact', or 'internal'."
                    .into(),
            )
        })?;

    match payload_type {
        "prompt" => {
            let text = input
                .get("text")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    input
                        .get("payload")
                        .and_then(|p| p.get("text"))
                        .and_then(|v| v.as_str())
                })
                .ok_or_else(|| {
                    ToolError::InvalidInput("missing 'text' for prompt payload".into())
                })?;
            let workspace_id = input
                .get("workspace_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok());
            let model_hint = input
                .get("model_hint")
                .and_then(|v| v.as_str())
                .map(String::from);
            Ok(JobPayload::Prompt {
                text: text.to_owned(),
                workspace_id,
                model_hint,
            })
        }
        "internal" => {
            let op = input
                .get("operation")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    input
                        .get("payload")
                        .and_then(|p| p.get("op"))
                        .and_then(|v| v.as_str())
                })
                .ok_or_else(|| {
                    ToolError::InvalidInput(
                        "missing 'operation' for internal payload. \
                         Options: MemoryPruning, SessionCleanup, VectorIndexOptimize, PluginAudit"
                            .into(),
                    )
                })?;
            let operation: sober_core::types::InternalOp =
                serde_json::from_value(serde_json::Value::String(op.to_owned())).map_err(|e| {
                    ToolError::InvalidInput(format!("invalid operation '{op}': {e}"))
                })?;
            Ok(JobPayload::Internal { operation })
        }
        "artifact" => {
            let blob_ref = input
                .get("blob_ref")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    input
                        .get("payload")
                        .and_then(|p| p.get("blob_ref"))
                        .and_then(|v| v.as_str())
                })
                .ok_or_else(|| {
                    ToolError::InvalidInput("missing 'blob_ref' for artifact payload".into())
                })?;
            let workspace_id_str = input
                .get("workspace_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    ToolError::InvalidInput("missing 'workspace_id' for artifact payload".into())
                })?;
            let workspace_id = Uuid::parse_str(workspace_id_str)
                .map_err(|e| ToolError::InvalidInput(format!("invalid workspace_id: {e}")))?;
            let artifact_type_str = input
                .get("artifact_type")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    input
                        .get("payload")
                        .and_then(|p| p.get("artifact_type"))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("Script");
            let artifact_type: sober_core::types::ArtifactType =
                serde_json::from_value(serde_json::Value::String(artifact_type_str.to_owned()))
                    .map_err(|e| ToolError::InvalidInput(format!("invalid artifact_type: {e}")))?;
            Ok(JobPayload::Artifact {
                blob_ref: blob_ref.to_owned(),
                artifact_type,
                workspace_id,
                env: std::collections::HashMap::new(),
            })
        }
        _ => Err(ToolError::InvalidInput(format!(
            "unknown payload_type '{payload_type}'. Use 'prompt', 'artifact', or 'internal'."
        ))),
    }
}

impl Tool for SchedulerTools {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "scheduler".to_owned(),
            description: "Manage scheduled jobs: create, list, get, cancel, pause, resume jobs \
                          and view job run history."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "get", "create", "cancel", "pause", "resume", "runs"],
                        "description": "The action to perform."
                    },
                    "job_id": {
                        "type": "string",
                        "description": "Job UUID (required for get, cancel, pause, resume, runs)."
                    },
                    "name": {
                        "type": "string",
                        "description": "Job name (required for create)."
                    },
                    "schedule": {
                        "type": "string",
                        "description": "Schedule expression, e.g. 'every: 30m' or '0 9 * * MON-FRI' (required for create)."
                    },
                    "payload_type": {
                        "type": "string",
                        "enum": ["prompt", "internal", "artifact"],
                        "description": "Job payload type (required for create). 'prompt' needs 'text', 'internal' needs 'operation', 'artifact' needs 'blob_ref' + 'workspace_id'."
                    },
                    "text": {
                        "type": "string",
                        "description": "Prompt text (required when payload_type is 'prompt')."
                    },
                    "operation": {
                        "type": "string",
                        "enum": ["MemoryPruning", "SessionCleanup", "VectorIndexOptimize", "PluginAudit"],
                        "description": "Internal operation (required when payload_type is 'internal')."
                    },
                    "blob_ref": {
                        "type": "string",
                        "description": "Content-addressed blob reference (required when payload_type is 'artifact')."
                    },
                    "artifact_type": {
                        "type": "string",
                        "enum": ["Wasm", "Script"],
                        "description": "Artifact type (optional for artifact payload, defaults to 'Script')."
                    },
                    "model_hint": {
                        "type": "string",
                        "description": "Optional model preference hint (for prompt payload)."
                    },
                    "owner_type": {
                        "type": "string",
                        "description": "Filter by owner type: 'user', 'group', 'system' (optional for list)."
                    },
                    "owner_id": {
                        "type": "string",
                        "description": "Filter by owner UUID (optional for list)."
                    },
                    "status": {
                        "type": "string",
                        "description": "Filter by a single status: 'active', 'paused', 'cancelled' (optional for list)."
                    },
                    "statuses": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Filter by multiple statuses, e.g. ['active', 'running'] (optional for list)."
                    },
                    "name_filter": {
                        "type": "string",
                        "description": "Filter jobs by name prefix (optional for list)."
                    },
                    "workspace_id": {
                        "type": "string",
                        "description": "Workspace UUID (optional for create and list)."
                    },
                    "conversation_id": {
                        "type": "string",
                        "description": "Conversation UUID to deliver results to (optional for create)."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max number of runs to return (optional for runs action)."
                    }
                },
                "required": ["action"]
            }),
            context_modifying: false,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}
