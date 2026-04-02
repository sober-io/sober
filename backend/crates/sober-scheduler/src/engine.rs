//! Tick engine — the scheduler's main loop.
//!
//! The engine wakes up on a configurable interval, finds due persistent jobs
//! from the database, and routes them: prompt jobs go to the agent via gRPC,
//! while artifact and internal jobs execute locally via the
//! [`JobExecutorRegistry`].

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use sober_core::error::AppError;
use sober_core::types::repo::{JobRepo, JobRunRepo};
use sober_core::types::{Job, JobId, JobStatus};
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tracing::{Instrument, error, info, instrument, warn};

use crate::executor::JobExecutorRegistry;
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
    executor_registry: Arc<JobExecutorRegistry>,
    tick_interval: Duration,
    concurrency_semaphore: Arc<tokio::sync::Semaphore>,
    paused: Arc<std::sync::atomic::AtomicBool>,
}

impl<J: JobRepo + 'static, R: JobRunRepo + 'static> TickEngine<J, R> {
    /// Creates a new tick engine.
    pub fn new(
        job_repo: Arc<J>,
        run_repo: Arc<R>,
        executor_registry: Arc<JobExecutorRegistry>,
        tick_interval: Duration,
        max_concurrent: usize,
    ) -> Self {
        Self {
            job_repo,
            run_repo,
            agent_client: Arc::new(RwLock::new(None)),
            executor_registry,
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
        info!("agent client connected");
    }

    /// Clear the agent gRPC client (called when the connection is lost).
    pub async fn clear_agent_client(&self) {
        let mut lock = self.agent_client.write().await;
        *lock = None;
        info!("agent client disconnected");
    }

    /// Get a clone of the shared agent client handle.
    pub fn agent_client(&self) -> SharedAgentClient {
        Arc::clone(&self.agent_client)
    }

    /// Get a clone of the executor registry.
    pub fn executor_registry(&self) -> Arc<JobExecutorRegistry> {
        Arc::clone(&self.executor_registry)
    }

    /// Whether the engine is paused.
    pub fn is_paused(&self) -> bool {
        self.paused.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Pause the tick engine (stops executing jobs but keeps running).
    #[instrument(skip(self))]
    pub fn pause(&self) {
        self.paused
            .store(true, std::sync::atomic::Ordering::Relaxed);
        info!("scheduler paused");
    }

    /// Resume the tick engine.
    #[instrument(skip(self))]
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
                    metrics::gauge!("sober_scheduler_paused")
                        .set(if self.is_paused() { 1.0 } else { 0.0 });
                    if self.is_paused() {
                        continue;
                    }
                    metrics::counter!("sober_scheduler_ticks_total").increment(1);
                    let span = tracing::info_span!("scheduler.tick");
                    self.tick().instrument(span).await;
                }
            }
        }
    }

    /// Execute a single tick: find and run due jobs.
    async fn tick(&self) {
        let tick_start = Instant::now();
        let now = Utc::now();

        let due_jobs = match self.job_repo.list_due(now).await {
            Ok(jobs) => jobs,
            Err(e) => {
                error!(error = %e, "failed to query due jobs");
                metrics::histogram!("sober_scheduler_tick_duration_seconds")
                    .record(tick_start.elapsed().as_secs_f64());
                metrics::histogram!("sober_scheduler_jobs_due_per_tick").record(0.0);
                return;
            }
        };

        metrics::histogram!("sober_scheduler_jobs_due_per_tick").record(due_jobs.len() as f64);

        if due_jobs.is_empty() {
            metrics::histogram!("sober_scheduler_tick_duration_seconds")
                .record(tick_start.elapsed().as_secs_f64());
            return;
        }

        // due_job_count is logged in the info! below; no inner span needed
        // (entered spans are !Send and cannot be held across .await)
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
            let executor_registry = Arc::clone(&self.executor_registry);

            let scheduled_at = job.next_run_at;

            let job_span = tracing::info_span!(
                "scheduler.job",
                job.name = %job.name,
                job.id = %job.id,
                job.type = job.payload["type"].as_str().unwrap_or("prompt"),
                otel.status_code = tracing::field::Empty,
            );
            handles.push(tokio::spawn(tracing::Instrument::instrument(
                async move {
                    let job_id = job.id;
                    let job_name = job.name.clone();
                    let job_start = Instant::now();

                    // Record scheduling lag (time between scheduled_at and actual execution).
                    let lag_secs =
                        (Utc::now() - scheduled_at).num_milliseconds().max(0) as f64 / 1000.0;
                    metrics::histogram!(
                        "sober_scheduler_job_lag_seconds",
                        "job_name" => job_name.clone(),
                    )
                    .record(lag_secs);

                    // Mark job as running
                    if let Err(e) = job_repo.update_status(job_id, JobStatus::Running).await {
                        error!(job = %job_name, error = %e, "failed to mark job as running");
                        metrics::counter!(
                            "sober_scheduler_job_executions_total",
                            "job_name" => job_name.clone(),
                            "status" => "error",
                        )
                        .increment(1);
                        metrics::histogram!(
                            "sober_scheduler_job_duration_seconds",
                            "job_name" => job_name,
                        )
                        .record(job_start.elapsed().as_secs_f64());
                        drop(permit);
                        return;
                    }

                    // Create a run record
                    let run = match run_repo.create(job_id).await {
                        Ok(r) => r,
                        Err(e) => {
                            error!(job = %job_name, error = %e, "failed to create job run");
                            let _ = job_repo.update_status(job_id, JobStatus::Active).await;
                            metrics::counter!(
                                "sober_scheduler_job_executions_total",
                                "job_name" => job_name.clone(),
                                "status" => "error",
                            )
                            .increment(1);
                            metrics::histogram!(
                                "sober_scheduler_job_duration_seconds",
                                "job_name" => job_name,
                            )
                            .record(job_start.elapsed().as_secs_f64());
                            drop(permit);
                            return;
                        }
                    };

                    // Route by payload type
                    let (result_bytes, error_msg) =
                        route_job(&agent_client, &executor_registry, &job).await;

                    let status_label = if error_msg.is_some() {
                        tracing::Span::current().record("otel.status_code", "ERROR");
                        "error"
                    } else {
                        tracing::Span::current().record("otel.status_code", "OK");
                        "success"
                    };
                    metrics::counter!(
                        "sober_scheduler_job_executions_total",
                        "job_name" => job_name.clone(),
                        "status" => status_label,
                    )
                    .increment(1);
                    metrics::histogram!(
                        "sober_scheduler_job_duration_seconds",
                        "job_name" => job_name.clone(),
                    )
                    .record(job_start.elapsed().as_secs_f64());

                    // Complete the run
                    if let Err(e) = run_repo
                        .complete(run.id, result_bytes, error_msg.clone(), None)
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

                    // Only restore to active if the job wasn't cancelled during execution.
                    let current = job_repo.get_by_id(job_id).await;
                    if let Ok(j) = current
                        && j.status == JobStatus::Running
                    {
                        let _ = job_repo.update_status(job_id, JobStatus::Active).await;
                    }

                    drop(permit);
                },
                job_span,
            )));
        }

        // Wait for all spawned tasks
        for handle in handles {
            let _ = handle.await;
        }

        metrics::histogram!("sober_scheduler_tick_duration_seconds")
            .record(tick_start.elapsed().as_secs_f64());
    }
}

/// Determine the job type from its JSON payload and route accordingly.
///
/// Payload format:
/// - `{"type": "prompt", ...}` or no `type` field → agent gRPC
/// - `{"type": "internal", "op": "<op>"}` → local executor
/// - `{"type": "artifact", "op": "<op>"}` → local executor
#[instrument(skip(agent_client, executor_registry, job), fields(job.id = %job.id, job.name = %job.name))]
async fn route_job(
    agent_client: &SharedAgentClient,
    executor_registry: &JobExecutorRegistry,
    job: &Job,
) -> (Vec<u8>, Option<String>) {
    let payload_type = job.payload["type"].as_str().unwrap_or("prompt");

    match payload_type {
        "prompt" => {
            tracing::info!(job.id = %job.id, job.name = %job.name, route = "agent", "routing job");
            execute_via_agent(agent_client, job).await
        }

        "internal" | "artifact" => {
            tracing::info!(job.id = %job.id, job.name = %job.name, route = "local", "routing job");
            let op = match job.payload["op"].as_str() {
                Some(op) => op,
                None => {
                    return (
                        vec![],
                        Some(format!("{payload_type} job missing 'op' field in payload")),
                    );
                }
            };

            match executor_registry.get(op) {
                Some(executor) => {
                    match executor.execute(job).await {
                        Ok(result) => {
                            // Fire-and-forget wake_agent with result summary
                            wake_agent(agent_client, job, &result.summary).await;

                            let summary_bytes = result.summary.into_bytes();
                            (summary_bytes, None)
                        }
                        Err(e) => {
                            let err_msg = format!("{op} executor failed: {e}");
                            warn!(job = %job.name, op, error = %e, "local executor failed");
                            (vec![], Some(err_msg))
                        }
                    }
                }
                None => {
                    let err_msg = format!("no executor registered for op '{op}'");
                    warn!(job = %job.name, op, "unknown operation");
                    (vec![], Some(err_msg))
                }
            }
        }

        other => {
            let err_msg = format!("unknown job payload type: {other}");
            warn!(job = %job.name, payload_type = other, "unknown payload type");
            (vec![], Some(err_msg))
        }
    }
}

/// Notify the agent about a completed local job execution (fire-and-forget).
#[instrument(level = "debug", skip(agent_client, job, summary), fields(job.id = %job.id))]
async fn wake_agent(agent_client: &SharedAgentClient, job: &Job, summary: &str) {
    let client = agent_client.read().await;
    let Some(client) = client.as_ref() else {
        return; // Agent not connected — skip notification
    };

    let payload_json = serde_json::json!({
        "summary": summary,
        "job_name": job.name,
    });

    let mut request = tonic::Request::new(agent_proto::WakeRequest {
        reason: "job_result".into(),
        caller_identity: "scheduler".into(),
        target_id: Some(job.id.as_uuid().to_string()),
        payload: Some(serde_json::to_vec(&payload_json).unwrap_or_default()),
    });
    sober_core::inject_trace_context(request.metadata_mut());

    let mut client = client.clone();
    if let Err(e) = client.wake_agent(request).await {
        warn!(job = %job.name, error = %e, "failed to wake agent after local execution");
    }
}

/// Execute a job via the agent's `ExecuteTask` RPC.
#[instrument(skip(agent_client, job), fields(job.id = %job.id))]
async fn execute_via_agent(
    agent_client: &SharedAgentClient,
    job: &sober_core::types::Job,
) -> (Vec<u8>, Option<String>) {
    let client = agent_client.read().await;
    let Some(client) = client.as_ref() else {
        tracing::debug!("agent client not connected, skipping gRPC dispatch");
        return (vec![], Some("agent client not connected".into()));
    };

    tracing::debug!("sending job to agent via gRPC");
    let mut request = tonic::Request::new(agent_proto::ExecuteTaskRequest {
        task_id: job.id.as_uuid().to_string(),
        task_type: "scheduled_job".into(),
        payload: serde_json::to_vec(&job.payload).unwrap_or_default(),
        caller_identity: "scheduler".into(),
        user_id: job.owner_id.map(|id| id.to_string()),
        conversation_id: job.conversation_id.map(|id| id.to_string()),
        workspace_id: job.workspace_id.map(|id| id.to_string()),
    });
    sober_core::inject_trace_context(request.metadata_mut());

    let mut client = client.clone();
    let result = match client.execute_task(request).await {
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
    };
    tracing::debug!("agent gRPC dispatch complete");
    result
}

/// Force-run a specific job immediately by ID.
#[instrument(skip(job_repo, run_repo, agent_client, executor_registry), fields(job.id = %job_id))]
pub async fn force_run_job<J: JobRepo + 'static, R: JobRunRepo + 'static>(
    job_repo: &Arc<J>,
    run_repo: &Arc<R>,
    agent_client: &SharedAgentClient,
    executor_registry: &Arc<JobExecutorRegistry>,
    job_id: JobId,
) -> Result<(), AppError> {
    let job = job_repo.get_by_id(job_id).await?;

    // Create run record
    let run = run_repo.create(job_id).await?;

    // Execute (route by payload type)
    let (result_bytes, error_msg) = route_job(agent_client, executor_registry, &job).await;

    // Complete the run
    run_repo
        .complete(run.id, result_bytes, error_msg, None)
        .await?;

    // Update last run
    let now = Utc::now();
    job_repo.mark_last_run(job_id, now).await?;

    Ok(())
}
