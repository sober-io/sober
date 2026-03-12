//! Agent tools for managing scheduled jobs via the scheduler gRPC service.

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
    fn authorize(
        &self,
        caller_user_id: Uuid,
        job: &scheduler_proto::Job,
        action: JobAction,
    ) -> Result<(), String> {
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
            ("system", _) => {
                return Err("System jobs cannot be managed via conversation".into());
            }
            _ => return Err("Unknown owner type".into()),
        }
        Ok(())
    }

    /// Creates a new scheduled job.
    pub async fn create_job(
        &self,
        name: &str,
        schedule: &str,
        payload: sober_core::types::JobPayload,
        caller_user_id: Uuid,
        workspace_id: Option<Uuid>,
        conversation_id: Option<Uuid>,
    ) -> Result<String, String> {
        let payload_bytes = payload.to_bytes().map_err(|e| e.to_string())?;

        // TODO: detect group workspace and set owner_type accordingly
        let owner_type = "user";

        let req = scheduler_proto::CreateJobRequest {
            name: name.into(),
            owner_type: owner_type.into(),
            owner_id: Some(caller_user_id.to_string()),
            schedule: schedule.into(),
            payload: payload_bytes,
            workspace_id: workspace_id.map(|id| id.to_string()).unwrap_or_default(),
            created_by: caller_user_id.to_string(),
            conversation_id: conversation_id.map(|id| id.to_string()).unwrap_or_default(),
        };

        let mut client = self.scheduler_client.write().await;
        let client = client.as_mut().ok_or("Scheduler not connected")?;
        let response = client.create_job(req).await.map_err(|e| e.to_string())?;
        let job = response.into_inner();

        Ok(format!(
            "Created job '{}' ({}). Next run: {}",
            job.name, job.id, job.next_run_at
        ))
    }

    /// Lists jobs with optional filters.
    pub async fn list_jobs(
        &self,
        owner_type: Option<&str>,
        owner_id: Option<Uuid>,
        status: Option<&str>,
        workspace_id: Option<Uuid>,
        name_filter: Option<&str>,
    ) -> Result<String, String> {
        let req = scheduler_proto::ListJobsRequest {
            owner_type: owner_type.map(|s| s.into()),
            owner_id: owner_id.map(|id| id.to_string()),
            status: status.map(|s| s.into()),
            workspace_id: workspace_id.map(|id| id.to_string()).unwrap_or_default(),
            name_filter: name_filter.unwrap_or_default().into(),
        };

        let mut client = self.scheduler_client.write().await;
        let client = client.as_mut().ok_or("Scheduler not connected")?;
        let response = client.list_jobs(req).await.map_err(|e| e.to_string())?;
        let jobs = response.into_inner().jobs;

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
    pub async fn get_job(&self, job_id: &str, caller_user_id: Uuid) -> Result<String, String> {
        let req = scheduler_proto::GetJobRequest {
            job_id: job_id.into(),
        };

        let mut client = self.scheduler_client.write().await;
        let client = client.as_mut().ok_or("Scheduler not connected")?;
        let response = client.get_job(req).await.map_err(|e| e.to_string())?;
        let job = response.into_inner();

        self.authorize(caller_user_id, &job, JobAction::View)?;

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
    pub async fn cancel_job(&self, job_id: &str, caller_user_id: Uuid) -> Result<String, String> {
        // Fetch the job first for authorization
        let get_req = scheduler_proto::GetJobRequest {
            job_id: job_id.into(),
        };
        let mut client = self.scheduler_client.write().await;
        let client = client.as_mut().ok_or("Scheduler not connected")?;
        let job = client
            .get_job(get_req)
            .await
            .map_err(|e| e.to_string())?
            .into_inner();

        self.authorize(caller_user_id, &job, JobAction::Cancel)?;

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
    pub async fn pause_job(&self, job_id: &str, caller_user_id: Uuid) -> Result<String, String> {
        let get_req = scheduler_proto::GetJobRequest {
            job_id: job_id.into(),
        };
        let mut client = self.scheduler_client.write().await;
        let client = client.as_mut().ok_or("Scheduler not connected")?;
        let job = client
            .get_job(get_req)
            .await
            .map_err(|e| e.to_string())?
            .into_inner();

        self.authorize(caller_user_id, &job, JobAction::Modify)?;

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
    pub async fn resume_job(&self, job_id: &str, caller_user_id: Uuid) -> Result<String, String> {
        let get_req = scheduler_proto::GetJobRequest {
            job_id: job_id.into(),
        };
        let mut client = self.scheduler_client.write().await;
        let client = client.as_mut().ok_or("Scheduler not connected")?;
        let job = client
            .get_job(get_req)
            .await
            .map_err(|e| e.to_string())?
            .into_inner();

        self.authorize(caller_user_id, &job, JobAction::Modify)?;

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
        limit: Option<u32>,
    ) -> Result<String, String> {
        // Fetch job for authorization
        let get_req = scheduler_proto::GetJobRequest {
            job_id: job_id.into(),
        };
        let mut client = self.scheduler_client.write().await;
        let client = client.as_mut().ok_or("Scheduler not connected")?;
        let job = client
            .get_job(get_req)
            .await
            .map_err(|e| e.to_string())?
            .into_inner();

        self.authorize(caller_user_id, &job, JobAction::View)?;

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
                    .map(|e| format!(" error: {e}"))
                    .unwrap_or_default();
                format!(
                    "- {} [{}] started: {} finished: {}{}",
                    r.id, r.status, r.started_at, finished, error_info
                )
            })
            .collect();

        Ok(lines.join("\n"))
    }
}
