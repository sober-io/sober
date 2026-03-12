//! Tick engine — the scheduler's main loop.
//!
//! The engine wakes up on a configurable interval, finds due persistent jobs
//! from the database, and executes them by calling the agent via gRPC.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sober_core::error::AppError;
use sober_core::types::repo::{JobRepo, JobRunRepo};
use sober_core::types::{JobId, JobStatus};
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tracing::{error, info};

use crate::grpc::agent_proto;
use crate::job::JobSchedule;

/// Shared agent gRPC client handle.
pub type SharedAgentClient = Arc<
    RwLock<
        Option<agent_proto::agent_service_client::AgentServiceClient<tonic::transport::Channel>>,
    >,
>;

/// The tick engine that drives autonomous job execution.
pub struct TickEngine<J: JobRepo, R: JobRunRepo> {
    job_repo: Arc<J>,
    run_repo: Arc<R>,
    agent_client: SharedAgentClient,
    tick_interval: Duration,
    concurrency_semaphore: Arc<tokio::sync::Semaphore>,
    paused: Arc<std::sync::atomic::AtomicBool>,
}

impl<J: JobRepo + 'static, R: JobRunRepo + 'static> TickEngine<J, R> {
    /// Creates a new tick engine.
    pub fn new(
        job_repo: Arc<J>,
        run_repo: Arc<R>,
        tick_interval: Duration,
        max_concurrent: usize,
    ) -> Self {
        Self {
            job_repo,
            run_repo,
            agent_client: Arc::new(RwLock::new(None)),
            tick_interval,
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(max_concurrent)),
            paused: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Set the agent gRPC client (called after connection is established).
    pub async fn set_agent_client(
        &self,
        client: agent_proto::agent_service_client::AgentServiceClient<tonic::transport::Channel>,
    ) {
        let mut lock = self.agent_client.write().await;
        *lock = Some(client);
    }

    /// Clear the agent gRPC client (called when the connection is lost).
    pub async fn clear_agent_client(&self) {
        let mut lock = self.agent_client.write().await;
        *lock = None;
    }

    /// Get a clone of the shared agent client handle.
    pub fn agent_client(&self) -> SharedAgentClient {
        Arc::clone(&self.agent_client)
    }

    /// Whether the engine is paused.
    pub fn is_paused(&self) -> bool {
        self.paused.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Pause the tick engine (stops executing jobs but keeps running).
    pub fn pause(&self) {
        self.paused
            .store(true, std::sync::atomic::Ordering::Relaxed);
        info!("scheduler paused");
    }

    /// Resume the tick engine.
    pub fn resume(&self) {
        self.paused
            .store(false, std::sync::atomic::Ordering::Relaxed);
        info!("scheduler resumed");
    }

    /// Run the main tick loop until the cancellation token is triggered.
    pub async fn run(&self, cancel: tokio_util::sync::CancellationToken) {
        info!(
            tick_interval_ms = self.tick_interval.as_millis() as u64,
            "tick engine started"
        );

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("tick engine shutting down");
                    break;
                }
                _ = tokio::time::sleep(self.tick_interval) => {
                    if self.is_paused() {
                        continue;
                    }
                    self.tick().await;
                }
            }
        }
    }

    /// Execute a single tick: find and run due jobs.
    async fn tick(&self) {
        let now = Utc::now();

        let due_jobs = match self.job_repo.list_due(now).await {
            Ok(jobs) => jobs,
            Err(e) => {
                error!(error = %e, "failed to query due jobs");
                return;
            }
        };

        if due_jobs.is_empty() {
            return;
        }

        info!(count = due_jobs.len(), "executing due jobs");

        let mut handles = Vec::new();

        for job in due_jobs {
            let permit = self.concurrency_semaphore.clone().acquire_owned().await;
            if permit.is_err() {
                break;
            }
            let permit = permit.unwrap();
            let job_repo = Arc::clone(&self.job_repo);
            let run_repo = Arc::clone(&self.run_repo);
            let agent_client = Arc::clone(&self.agent_client);

            handles.push(tokio::spawn(async move {
                let job_id = job.id;
                let job_name = job.name.clone();

                // Mark job as running
                if let Err(e) = job_repo.update_status(job_id, JobStatus::Running).await {
                    error!(job = %job_name, error = %e, "failed to mark job as running");
                    drop(permit);
                    return;
                }

                // Create a run record
                let run = match run_repo.create(job_id).await {
                    Ok(r) => r,
                    Err(e) => {
                        error!(job = %job_name, error = %e, "failed to create job run");
                        let _ = job_repo.update_status(job_id, JobStatus::Active).await;
                        drop(permit);
                        return;
                    }
                };

                // Execute via agent gRPC
                let (result_bytes, error_msg) = execute_via_agent(&agent_client, &job).await;

                // Complete the run
                if let Err(e) = run_repo
                    .complete(run.id, result_bytes, error_msg.clone())
                    .await
                {
                    error!(job = %job_name, error = %e, "failed to complete job run");
                }

                // Update job state
                let now = Utc::now();
                let _ = job_repo.mark_last_run(job_id, now).await;

                // Calculate next run
                if let Ok(schedule) = JobSchedule::parse(&job.schedule)
                    && let Some(next) = schedule.next_run_after(now)
                {
                    let _ = job_repo.update_next_run(job_id, next).await;
                }

                // Set back to active
                let _ = job_repo.update_status(job_id, JobStatus::Active).await;

                drop(permit);
            }));
        }

        // Wait for all spawned tasks
        for handle in handles {
            let _ = handle.await;
        }
    }
}

/// Execute a job via the agent's `ExecuteTask` RPC.
async fn execute_via_agent(
    agent_client: &SharedAgentClient,
    job: &sober_core::types::Job,
) -> (Vec<u8>, Option<String>) {
    let client = agent_client.read().await;
    let Some(client) = client.as_ref() else {
        return (vec![], Some("agent client not connected".into()));
    };

    let request = agent_proto::ExecuteTaskRequest {
        task_id: job.id.as_uuid().to_string(),
        task_type: "scheduled_job".into(),
        payload: job.payload_bytes.clone(),
        caller_identity: "scheduler".into(),
        user_id: job.owner_id.map(|id| id.to_string()),
        conversation_id: None,
        workspace_id: None,
    };

    let mut client = client.clone();
    match client.execute_task(request).await {
        Ok(response) => {
            let mut stream = response.into_inner();
            let mut collected = Vec::new();
            let mut last_error = None;

            while let Some(event_result) = stream.next().await {
                match event_result {
                    Ok(event) => {
                        if let Some(inner) = event.event {
                            match inner {
                                agent_proto::agent_event::Event::Error(e) => {
                                    last_error = Some(e.message);
                                }
                                agent_proto::agent_event::Event::TextDelta(td) => {
                                    collected.extend_from_slice(td.content.as_bytes());
                                }
                                agent_proto::agent_event::Event::Done(_) => {
                                    // Stream complete
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(status) => {
                        last_error = Some(format!("stream error: {status}"));
                        break;
                    }
                }
            }

            (collected, last_error)
        }
        Err(status) => (vec![], Some(format!("gRPC call failed: {status}"))),
    }
}

/// Force-run a specific job immediately by ID.
pub async fn force_run_job<J: JobRepo + 'static, R: JobRunRepo + 'static>(
    job_repo: &Arc<J>,
    run_repo: &Arc<R>,
    agent_client: &SharedAgentClient,
    job_id: JobId,
) -> Result<(), AppError> {
    let job = job_repo.get_by_id(job_id).await?;

    // Create run record
    let run = run_repo.create(job_id).await?;

    // Execute
    let (result_bytes, error_msg) = execute_via_agent(agent_client, &job).await;

    // Complete the run
    run_repo.complete(run.id, result_bytes, error_msg).await?;

    // Update last run
    let now = Utc::now();
    job_repo.mark_last_run(job_id, now).await?;

    Ok(())
}
