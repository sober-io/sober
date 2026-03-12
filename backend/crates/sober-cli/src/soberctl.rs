//! `soberctl` — runtime CLI for agent inspection, scheduler control, and
//! live health checks via Unix domain sockets.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use hyper_util::rt::TokioIo;
use tonic::transport::{Endpoint, Uri};
use tower::service_fn;

/// Generated protobuf types for the scheduler gRPC service.
mod scheduler_proto {
    tonic::include_proto!("sober.scheduler.v1");
}

/// soberctl — runtime admin CLI for Sõber services.
#[derive(Debug, Parser)]
#[command(name = "soberctl", version, about)]
struct Ctl {
    /// Subcommand to execute.
    #[command(subcommand)]
    command: CtlCommand,
}

/// Top-level soberctl commands.
#[derive(Debug, Subcommand)]
enum CtlCommand {
    /// Manage the scheduler.
    #[command(subcommand)]
    Scheduler(SchedulerCommand),
}

/// Scheduler management subcommands.
#[derive(Debug, Subcommand)]
enum SchedulerCommand {
    /// Check scheduler health.
    Health {
        /// Path to scheduler socket.
        #[arg(long, default_value = "/run/sober/scheduler.sock")]
        socket: String,
    },

    /// List scheduled jobs.
    List {
        /// Filter by owner type (system, user, agent).
        #[arg(long)]
        owner_type: Option<String>,

        /// Filter by status (active, paused, cancelled, running).
        #[arg(long)]
        status: Option<String>,

        /// Path to scheduler socket.
        #[arg(long, default_value = "/run/sober/scheduler.sock")]
        socket: String,
    },

    /// Pause the scheduler tick engine.
    Pause {
        /// Path to scheduler socket.
        #[arg(long, default_value = "/run/sober/scheduler.sock")]
        socket: String,
    },

    /// Resume the scheduler tick engine.
    Resume {
        /// Path to scheduler socket.
        #[arg(long, default_value = "/run/sober/scheduler.sock")]
        socket: String,
    },

    /// Force-run a specific job immediately.
    Run {
        /// Job ID (UUID) to run.
        job_id: String,

        /// Path to scheduler socket.
        #[arg(long, default_value = "/run/sober/scheduler.sock")]
        socket: String,
    },

    /// Cancel a scheduled job.
    Cancel {
        /// Job ID (UUID) to cancel.
        job_id: String,

        /// Path to scheduler socket.
        #[arg(long, default_value = "/run/sober/scheduler.sock")]
        socket: String,
    },

    /// Show details for a specific job.
    Get {
        /// Job ID (UUID).
        job_id: String,

        /// Path to scheduler socket.
        #[arg(long, default_value = "/run/sober/scheduler.sock")]
        socket: String,
    },

    /// List runs for a specific job.
    Runs {
        /// Job ID (UUID).
        job_id: String,

        /// Maximum number of runs to return.
        #[arg(long, default_value_t = 20)]
        limit: u32,

        /// Path to scheduler socket.
        #[arg(long, default_value = "/run/sober/scheduler.sock")]
        socket: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let ctl = Ctl::parse();

    match ctl.command {
        CtlCommand::Scheduler(cmd) => handle_scheduler(cmd).await,
    }
}

/// Connect to the scheduler gRPC service over a Unix domain socket.
async fn connect_scheduler(
    socket_path: &str,
) -> Result<
    scheduler_proto::scheduler_service_client::SchedulerServiceClient<tonic::transport::Channel>,
> {
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

    Ok(scheduler_proto::scheduler_service_client::SchedulerServiceClient::new(channel))
}

/// Handle scheduler subcommands.
async fn handle_scheduler(cmd: SchedulerCommand) -> Result<()> {
    match cmd {
        SchedulerCommand::Health { socket } => {
            let mut client = connect_scheduler(&socket).await?;
            let resp = client
                .health(scheduler_proto::HealthRequest {})
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
            let mut client = connect_scheduler(&socket).await?;
            let resp = client
                .list_jobs(scheduler_proto::ListJobsRequest {
                    owner_type,
                    owner_id: None,
                    status,
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
            let mut client = connect_scheduler(&socket).await?;
            client
                .pause_scheduler(scheduler_proto::PauseRequest {})
                .await
                .context("failed to pause scheduler")?;
            println!("scheduler paused");
        }

        SchedulerCommand::Resume { socket } => {
            let mut client = connect_scheduler(&socket).await?;
            client
                .resume_scheduler(scheduler_proto::ResumeRequest {})
                .await
                .context("failed to resume scheduler")?;
            println!("scheduler resumed");
        }

        SchedulerCommand::Run { job_id, socket } => {
            let mut client = connect_scheduler(&socket).await?;
            let resp = client
                .force_run(scheduler_proto::ForceRunRequest {
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
            let mut client = connect_scheduler(&socket).await?;
            client
                .cancel_job(scheduler_proto::CancelJobRequest {
                    job_id: job_id.clone(),
                })
                .await
                .context("failed to cancel job")?;
            println!("job {job_id} cancelled");
        }

        SchedulerCommand::Get { job_id, socket } => {
            let mut client = connect_scheduler(&socket).await?;
            let resp = client
                .get_job(scheduler_proto::GetJobRequest {
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
            let mut client = connect_scheduler(&socket).await?;
            let resp = client
                .list_job_runs(scheduler_proto::ListJobRunsRequest {
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
fn print_job(job: &scheduler_proto::Job) {
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
fn print_job_run(run: &scheduler_proto::JobRun) {
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
