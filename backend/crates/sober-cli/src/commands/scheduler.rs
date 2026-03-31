//! Scheduler management commands via gRPC over Unix domain socket.

use anyhow::{Context, Result};
use hyper_util::rt::TokioIo;
use tonic::transport::{Endpoint, Uri};
use tower::service_fn;

/// Generated protobuf types for the scheduler gRPC service.
pub(super) mod proto {
    tonic::include_proto!("sober.scheduler.v1");
}

use proto::scheduler_service_client::SchedulerServiceClient;
use proto::{
    CancelJobRequest, ForceRunRequest, GetJobRequest, HealthRequest, Job, JobRun,
    ListJobRunsRequest, ListJobsRequest, PauseRequest, ResumeRequest,
};

use crate::cli::SchedulerCommand;

/// Connect to the scheduler gRPC service over a Unix domain socket.
pub(super) async fn connect(
    socket_path: &str,
) -> Result<SchedulerServiceClient<tonic::transport::Channel>> {
    let path = std::path::PathBuf::from(socket_path);
    if !path.exists() {
        anyhow::bail!(
            "scheduler socket not found at {} — is sober-scheduler running?",
            socket_path
        );
    }

    let channel = Endpoint::try_from("http://[::]:50051")
        .context("invalid endpoint URI")?
        .connect_with_connector(service_fn(move |_: Uri| {
            let path = path.clone();
            async move {
                let stream = tokio::net::UnixStream::connect(path).await?;
                Ok::<_, std::io::Error>(TokioIo::new(stream))
            }
        }))
        .await
        .with_context(|| format!("failed to connect to scheduler at {socket_path}"))?;

    Ok(SchedulerServiceClient::new(channel))
}

/// Execute a scheduler subcommand (requires running sober-scheduler).
pub async fn handle(cmd: SchedulerCommand) -> Result<()> {
    match cmd {
        SchedulerCommand::Health { socket } => {
            let mut client = connect(&socket).await?;
            let resp = client
                .health(HealthRequest {})
                .await
                .context("health check failed")?;
            let health = resp.into_inner();
            println!("healthy: {}", health.healthy);
            println!("version: {}", health.version);
        }

        SchedulerCommand::List {
            owner_type,
            status,
            socket,
        } => {
            let mut client = connect(&socket).await?;
            let resp = client
                .list_jobs(ListJobsRequest {
                    owner_type,
                    owner_id: None,
                    statuses: status.map(|s| vec![s]).unwrap_or_default(),
                    workspace_id: String::new(),
                    name_filter: String::new(),
                })
                .await
                .context("failed to list jobs")?;
            let jobs = resp.into_inner().jobs;
            if jobs.is_empty() {
                println!("no jobs found");
                return Ok(());
            }
            for job in &jobs {
                print_job(job);
                println!();
            }
            println!("{} job(s) total", jobs.len());
        }

        SchedulerCommand::Pause { socket } => {
            let mut client = connect(&socket).await?;
            client
                .pause_scheduler(PauseRequest {})
                .await
                .context("failed to pause scheduler")?;
            println!("scheduler paused");
        }

        SchedulerCommand::Resume { socket } => {
            let mut client = connect(&socket).await?;
            client
                .resume_scheduler(ResumeRequest {})
                .await
                .context("failed to resume scheduler")?;
            println!("scheduler resumed");
        }

        SchedulerCommand::Run { job_id, socket } => {
            let mut client = connect(&socket).await?;
            let resp = client
                .force_run(ForceRunRequest {
                    job_id: job_id.clone(),
                })
                .await
                .context("failed to trigger force run")?;
            if resp.into_inner().accepted {
                println!("force run accepted for job {job_id}");
            } else {
                println!("force run rejected for job {job_id}");
            }
        }

        SchedulerCommand::Cancel { job_id, socket } => {
            let mut client = connect(&socket).await?;
            client
                .cancel_job(CancelJobRequest {
                    job_id: job_id.clone(),
                })
                .await
                .context("failed to cancel job")?;
            println!("job {job_id} cancelled");
        }

        SchedulerCommand::Get { job_id, socket } => {
            let mut client = connect(&socket).await?;
            let resp = client
                .get_job(GetJobRequest {
                    job_id: job_id.clone(),
                })
                .await
                .context("failed to get job")?;
            print_job(&resp.into_inner());
        }

        SchedulerCommand::Runs {
            job_id,
            limit,
            socket,
        } => {
            let mut client = connect(&socket).await?;
            let resp = client
                .list_job_runs(ListJobRunsRequest {
                    job_id: job_id.clone(),
                    limit: Some(limit),
                })
                .await
                .context("failed to list job runs")?;
            let runs = resp.into_inner().runs;
            if runs.is_empty() {
                println!("no runs found for job {job_id}");
                return Ok(());
            }
            for run in &runs {
                print_job_run(run);
                println!();
            }
            println!("{} run(s) total", runs.len());
        }
    }

    Ok(())
}

/// Pretty-print a job.
fn print_job(job: &Job) {
    println!("  id:           {}", job.id);
    println!("  name:         {}", job.name);
    println!("  status:       {}", job.status);
    println!("  schedule:     {}", job.schedule);
    println!("  owner_type:   {}", job.owner_type);
    if let Some(ref oid) = job.owner_id {
        println!("  owner_id:     {oid}");
    }
    println!("  next_run_at:  {}", job.next_run_at);
    if let Some(ref last) = job.last_run_at {
        println!("  last_run_at:  {last}");
    }
    println!("  created_at:   {}", job.created_at);
}

/// Pretty-print a job run.
fn print_job_run(run: &JobRun) {
    println!("  id:          {}", run.id);
    println!("  job_id:      {}", run.job_id);
    println!("  status:      {}", run.status);
    println!("  started_at:  {}", run.started_at);
    if let Some(ref finished) = run.finished_at {
        println!("  finished_at: {finished}");
    }
    if let Some(ref err) = run.error {
        println!("  error:       {err}");
    }
}
