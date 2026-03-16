//! gRPC server and client setup for the scheduler.

use std::sync::Arc;

use chrono::Utc;
use sober_core::error::AppError;
use sober_core::types::repo::{JobRepo, JobRunRepo};
use sober_core::types::{CreateJob, JobId, JobStatus};
use tonic::{Request, Response, Status};
use tracing::error;

use crate::engine::{TickEngine, force_run_job};
use crate::job::JobSchedule;

/// Generated protobuf types for the scheduler gRPC service.
pub mod scheduler_proto {
    tonic::include_proto!("sober.scheduler.v1");
}

/// Generated protobuf types for the agent gRPC service (client-side).
pub mod agent_proto {
    tonic::include_proto!("sober.agent.v1");
}

/// gRPC service implementation for the scheduler.
pub struct SchedulerGrpcService<J: JobRepo, R: JobRunRepo> {
    job_repo: Arc<J>,
    run_repo: Arc<R>,
    engine: Arc<TickEngine<J, R>>,
}

impl<J: JobRepo, R: JobRunRepo> SchedulerGrpcService<J, R> {
    /// Creates a new gRPC service.
    pub fn new(job_repo: Arc<J>, run_repo: Arc<R>, engine: Arc<TickEngine<J, R>>) -> Self {
        Self {
            job_repo,
            run_repo,
            engine,
        }
    }
}

/// Convert a domain Job to its proto representation.
fn job_to_proto(job: sober_core::types::Job) -> scheduler_proto::Job {
    scheduler_proto::Job {
        id: job.id.as_uuid().to_string(),
        name: job.name,
        owner_type: job.owner_type,
        owner_id: job.owner_id.map(|id| id.to_string()),
        schedule: job.schedule,
        payload: serde_json::to_vec(&job.payload).unwrap_or_default(),
        status: job_status_str(job.status).into(),
        next_run_at: job.next_run_at.to_rfc3339(),
        last_run_at: job.last_run_at.map(|t| t.to_rfc3339()),
        created_at: job.created_at.to_rfc3339(),
        workspace_id: job
            .workspace_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        created_by: job.created_by.map(|id| id.to_string()).unwrap_or_default(),
        conversation_id: job
            .conversation_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
    }
}

/// Parse an optional UUID from a proto string field (empty = None).
fn parse_optional_uuid(s: &str) -> Result<Option<uuid::Uuid>, Status> {
    if s.is_empty() {
        Ok(None)
    } else {
        uuid::Uuid::parse_str(s)
            .map(Some)
            .map_err(|e| Status::invalid_argument(e.to_string()))
    }
}

/// Convert a domain JobRun to its proto representation.
fn job_run_to_proto(run: sober_core::types::JobRun) -> scheduler_proto::JobRun {
    scheduler_proto::JobRun {
        id: run.id.as_uuid().to_string(),
        job_id: run.job_id.as_uuid().to_string(),
        started_at: run.started_at.to_rfc3339(),
        finished_at: run.finished_at.map(|t| t.to_rfc3339()),
        status: run.status,
        result: run.result,
        error: run.error,
    }
}

/// Convert a JobStatus enum to its string representation.
fn job_status_str(status: sober_core::types::JobStatus) -> &'static str {
    match status {
        sober_core::types::JobStatus::Active => "active",
        sober_core::types::JobStatus::Paused => "paused",
        sober_core::types::JobStatus::Cancelled => "cancelled",
        sober_core::types::JobStatus::Running => "running",
    }
}

/// Map an AppError to a tonic Status.
fn app_error_to_status(e: AppError) -> Status {
    match e {
        AppError::NotFound(msg) => Status::not_found(msg),
        AppError::Validation(msg) => Status::invalid_argument(msg),
        other => {
            error!(error = %other, "internal scheduler error");
            Status::internal(other.to_string())
        }
    }
}

#[tonic::async_trait]
impl<J: JobRepo + 'static, R: JobRunRepo + 'static>
    scheduler_proto::scheduler_service_server::SchedulerService for SchedulerGrpcService<J, R>
{
    async fn create_job(
        &self,
        request: Request<scheduler_proto::CreateJobRequest>,
    ) -> Result<Response<scheduler_proto::Job>, Status> {
        let req = request.into_inner();

        // Validate schedule
        let schedule = JobSchedule::parse(&req.schedule)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        let now = Utc::now();
        let next_run_at = schedule
            .next_run_after(now)
            .ok_or_else(|| Status::invalid_argument("schedule produces no future runs"))?;

        let owner_id = req
            .owner_id
            .map(|s| {
                s.parse::<uuid::Uuid>()
                    .map_err(|_| Status::invalid_argument("invalid owner_id UUID"))
            })
            .transpose()?;

        // Deserialize the gRPC bytes payload as JSON.
        let payload_json = serde_json::from_slice::<serde_json::Value>(&req.payload)
            .map_err(|e| Status::invalid_argument(format!("payload must be valid JSON: {e}")))?;

        let workspace_id = parse_optional_uuid(&req.workspace_id)?;
        let created_by = parse_optional_uuid(&req.created_by)?;
        let conversation_id = parse_optional_uuid(&req.conversation_id)?;

        let input = CreateJob {
            name: req.name,
            schedule: req.schedule,
            payload: payload_json,
            owner_type: req.owner_type,
            owner_id,
            workspace_id,
            created_by,
            conversation_id,
            next_run_at,
        };

        let job = self
            .job_repo
            .create(input)
            .await
            .map_err(app_error_to_status)?;

        Ok(Response::new(job_to_proto(job)))
    }

    async fn cancel_job(
        &self,
        request: Request<scheduler_proto::CancelJobRequest>,
    ) -> Result<Response<scheduler_proto::CancelJobResponse>, Status> {
        let job_id = parse_job_id(&request.into_inner().job_id)?;
        self.job_repo
            .cancel(job_id)
            .await
            .map_err(app_error_to_status)?;
        Ok(Response::new(scheduler_proto::CancelJobResponse {}))
    }

    async fn list_jobs(
        &self,
        request: Request<scheduler_proto::ListJobsRequest>,
    ) -> Result<Response<scheduler_proto::ListJobsResponse>, Status> {
        let req = request.into_inner();

        let owner_id = req
            .owner_id
            .map(|s| {
                s.parse::<uuid::Uuid>()
                    .map_err(|_| Status::invalid_argument("invalid owner_id UUID"))
            })
            .transpose()?;

        let workspace_id = parse_optional_uuid(&req.workspace_id)?;
        let name_filter = if req.name_filter.is_empty() {
            None
        } else {
            Some(req.name_filter.as_str())
        };

        let jobs = self
            .job_repo
            .list_filtered(
                req.owner_type.as_deref(),
                owner_id,
                &req.statuses,
                workspace_id,
                name_filter,
                None,
            )
            .await
            .map_err(app_error_to_status)?;

        Ok(Response::new(scheduler_proto::ListJobsResponse {
            jobs: jobs.into_iter().map(job_to_proto).collect(),
        }))
    }

    async fn get_job(
        &self,
        request: Request<scheduler_proto::GetJobRequest>,
    ) -> Result<Response<scheduler_proto::Job>, Status> {
        let job_id = parse_job_id(&request.into_inner().job_id)?;
        let job = self
            .job_repo
            .get_by_id(job_id)
            .await
            .map_err(app_error_to_status)?;
        Ok(Response::new(job_to_proto(job)))
    }

    async fn list_job_runs(
        &self,
        request: Request<scheduler_proto::ListJobRunsRequest>,
    ) -> Result<Response<scheduler_proto::ListJobRunsResponse>, Status> {
        let req = request.into_inner();
        let job_id = parse_job_id(&req.job_id)?;
        let limit = req.limit.unwrap_or(20);

        let runs = self
            .run_repo
            .list_by_job(job_id, limit)
            .await
            .map_err(app_error_to_status)?;

        Ok(Response::new(scheduler_proto::ListJobRunsResponse {
            runs: runs.into_iter().map(job_run_to_proto).collect(),
        }))
    }

    async fn pause_scheduler(
        &self,
        _request: Request<scheduler_proto::PauseRequest>,
    ) -> Result<Response<scheduler_proto::PauseResponse>, Status> {
        self.engine.pause();
        Ok(Response::new(scheduler_proto::PauseResponse {}))
    }

    async fn resume_scheduler(
        &self,
        _request: Request<scheduler_proto::ResumeRequest>,
    ) -> Result<Response<scheduler_proto::ResumeResponse>, Status> {
        self.engine.resume();
        Ok(Response::new(scheduler_proto::ResumeResponse {}))
    }

    async fn force_run(
        &self,
        request: Request<scheduler_proto::ForceRunRequest>,
    ) -> Result<Response<scheduler_proto::ForceRunResponse>, Status> {
        let job_id = parse_job_id(&request.into_inner().job_id)?;

        // Spawn force-run in background so we don't block the RPC
        let job_repo = Arc::clone(&self.job_repo);
        let run_repo = Arc::clone(&self.run_repo);
        let agent_client = self.engine.agent_client();
        let executor_registry = self.engine.executor_registry();

        tokio::spawn(async move {
            if let Err(e) = force_run_job(
                &job_repo,
                &run_repo,
                &agent_client,
                &executor_registry,
                job_id,
            )
            .await
            {
                error!(job_id = %job_id, error = %e, "force run failed");
            }
        });

        Ok(Response::new(scheduler_proto::ForceRunResponse {
            accepted: true,
        }))
    }

    async fn pause_job(
        &self,
        request: Request<scheduler_proto::PauseJobRequest>,
    ) -> Result<Response<scheduler_proto::PauseJobResponse>, Status> {
        let job_id = parse_job_id(&request.into_inner().job_id)?;

        self.job_repo
            .update_status(job_id, JobStatus::Paused)
            .await
            .map_err(app_error_to_status)?;

        let job = self
            .job_repo
            .get_by_id(job_id)
            .await
            .map_err(app_error_to_status)?;

        Ok(Response::new(scheduler_proto::PauseJobResponse {
            job: Some(job_to_proto(job)),
        }))
    }

    async fn resume_job(
        &self,
        request: Request<scheduler_proto::ResumeJobRequest>,
    ) -> Result<Response<scheduler_proto::ResumeJobResponse>, Status> {
        let job_id = parse_job_id(&request.into_inner().job_id)?;

        self.job_repo
            .update_status(job_id, JobStatus::Active)
            .await
            .map_err(app_error_to_status)?;

        // Recalculate next_run_at from now
        let job = self
            .job_repo
            .get_by_id(job_id)
            .await
            .map_err(app_error_to_status)?;

        let schedule =
            JobSchedule::parse(&job.schedule).map_err(|e| Status::internal(e.to_string()))?;
        let next_run = schedule
            .next_run_after(Utc::now())
            .ok_or_else(|| Status::internal("could not calculate next run"))?;

        self.job_repo
            .update_next_run(job_id, next_run)
            .await
            .map_err(app_error_to_status)?;

        let updated_job = self
            .job_repo
            .get_by_id(job_id)
            .await
            .map_err(app_error_to_status)?;

        Ok(Response::new(scheduler_proto::ResumeJobResponse {
            job: Some(job_to_proto(updated_job)),
        }))
    }

    async fn health(
        &self,
        _request: Request<scheduler_proto::HealthRequest>,
    ) -> Result<Response<scheduler_proto::HealthResponse>, Status> {
        Ok(Response::new(scheduler_proto::HealthResponse {
            healthy: true,
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }))
    }
}

/// Parse a UUID string into a JobId.
fn parse_job_id(s: &str) -> Result<JobId, Status> {
    s.parse::<uuid::Uuid>()
        .map(JobId::from_uuid)
        .map_err(|_| Status::invalid_argument("invalid job_id UUID"))
}
