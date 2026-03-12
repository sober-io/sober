//! Tick engine — the scheduler's main loop.
//!
//! The engine wakes up on a configurable interval, finds due jobs (both
//! persistent from the database and ephemeral from an in-memory registry),
//! and executes them by calling the agent via gRPC or running a local handler.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sober_core::error::AppError;
use sober_core::types::repo::{JobRepo, JobRunRepo};
use sober_core::types::{JobId, JobStatus};
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tracing::{error, info, warn};

use crate::grpc::agent_proto;
use crate::job::JobSchedule;

/// Shared agent gRPC client handle.
pub type SharedAgentClient = Arc<
    RwLock<
        Option<agent_proto::agent_service_client::AgentServiceClient<tonic::transport::Channel>>,
    >,
>;

/// A boxed async handler for ephemeral system jobs.
type SystemJobHandler =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send>> + Send + Sync>;

/// An ephemeral system job registered at startup.
struct SystemJob {
    name: String,
    schedule: JobSchedule,
    handler: SystemJobHandler,
    next_run_at: chrono::DateTime<Utc>,
}

/// The tick engine that drives autonomous job execution.
pub struct TickEngine<J: JobRepo, R: JobRunRepo> {
    job_repo: Arc<J>,
    run_repo: Arc<R>,
    agent_client: SharedAgentClient,
    system_jobs: RwLock<HashMap<String, SystemJob>>,
    tick_interval: Duration,
    max_concurrent: usize,
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
            system_jobs: RwLock::new(HashMap::new()),
            tick_interval,
            max_concurrent,
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

    /// Get a clone of the shared agent client handle.
    pub fn agent_client(&self) -> SharedAgentClient {
        Arc::clone(&self.agent_client)
    }

    /// Register an ephemeral system job that runs on a schedule.
    ///
    /// System jobs live in memory only — they re-register on startup.
    pub async fn register_system_job<F, Fut>(
        &self,
        name: impl Into<String>,
        schedule: JobSchedule,
        handler: F,
    ) where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<u8>, String>> + Send + 'static,
    {
        let name = name.into();
        let now = Utc::now();
        let next_run_at = schedule.next_run_after(now).unwrap_or(now);
        let handler: SystemJobHandler = Arc::new(move || Box::pin(handler()));

        let mut jobs = self.system_jobs.write().await;
        jobs.insert(
            name.clone(),
            SystemJob {
                name,
                schedule,
                handler,
                next_run_at,
            },
        );
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

        // Collect due system jobs
        let system_tasks = self.collect_due_system_jobs(now).await;

        // Collect due persistent jobs
        let persistent_tasks = match self.job_repo.list_due(now).await {
            Ok(jobs) => jobs,
            Err(e) => {
                error!(error = %e, "failed to query due jobs");
                Vec::new()
            }
        };

        let total = system_tasks.len() + persistent_tasks.len();
        if total == 0 {
            return;
        }

        info!(
            system = system_tasks.len(),
            persistent = persistent_tasks.len(),
            "executing due jobs"
        );

        // Limit concurrency
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.max_concurrent));
        let mut handles = Vec::new();

        // Spawn system jobs
        for (name, handler) in system_tasks {
            let permit = semaphore.clone().acquire_owned().await;
            if permit.is_err() {
                break;
            }
            let permit = permit.unwrap();
            handles.push(tokio::spawn(async move {
                let result = (handler)().await;
                if let Err(e) = &result {
                    warn!(job = %name, error = %e, "system job failed");
                }
                drop(permit);
            }));
        }

        // Spawn persistent jobs
        for job in persistent_tasks {
            let permit = semaphore.clone().acquire_owned().await;
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

                // Wake agent if configured
                if job.notify_agent {
                    wake_agent(&agent_client, job_id, error_msg.is_some()).await;
                }

                drop(permit);
            }));
        }

        // Wait for all spawned tasks
        for handle in handles {
            let _ = handle.await;
        }
    }

    /// Collect due system jobs and advance their next_run_at.
    async fn collect_due_system_jobs(
        &self,
        now: chrono::DateTime<Utc>,
    ) -> Vec<(String, SystemJobHandler)> {
        let mut jobs = self.system_jobs.write().await;
        let mut due = Vec::new();

        for job in jobs.values_mut() {
            if job.next_run_at <= now {
                due.push((job.name.clone(), Arc::clone(&job.handler)));
                // Advance to next run
                if let Some(next) = job.schedule.next_run_after(now) {
                    job.next_run_at = next;
                }
            }
        }

        due
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

/// Wake the agent after a job completes (if notify_agent is set).
async fn wake_agent(agent_client: &SharedAgentClient, job_id: JobId, failed: bool) {
    let client = agent_client.read().await;
    let Some(client) = client.as_ref() else {
        warn!("cannot wake agent: client not connected");
        return;
    };

    let reason = if failed {
        "job_failed"
    } else {
        "job_completed"
    };

    let mut client = client.clone();
    match client
        .wake_agent(agent_proto::WakeRequest {
            reason: reason.into(),
            caller_identity: "scheduler".into(),
            target_id: Some(job_id.as_uuid().to_string()),
        })
        .await
    {
        Ok(_) => info!(job_id = %job_id, "agent woken after job completion"),
        Err(e) => warn!(job_id = %job_id, error = %e, "failed to wake agent"),
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
    run_repo
        .complete(run.id, result_bytes, error_msg.clone())
        .await?;

    // Update last run
    let now = Utc::now();
    job_repo.mark_last_run(job_id, now).await?;

    // Wake agent if configured
    if job.notify_agent {
        wake_agent(agent_client, job_id, error_msg.is_some()).await;
    }

    Ok(())
}
