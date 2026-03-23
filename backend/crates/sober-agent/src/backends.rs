//! Concrete backend implementations wired in `main.rs`.
//!
//! These structs implement the object-safe backend traits defined in
//! `sober-plugin` using services only available in the agent process
//! (e.g. the scheduler gRPC client).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use sober_core::types::ids::PluginId;
use sober_plugin::backends::ScheduleBackend;

use crate::SharedSchedulerClient;
use crate::grpc::scheduler_proto;

/// Scheduler backend that delegates to the scheduler gRPC service.
///
/// Uses the shared scheduler client handle which may not be connected yet.
/// Returns an error if the scheduler is unavailable.
pub struct GrpcScheduleBackend {
    client: SharedSchedulerClient,
}

impl GrpcScheduleBackend {
    /// Creates a new gRPC-backed schedule backend.
    pub fn new(client: SharedSchedulerClient) -> Self {
        Self { client }
    }
}

impl ScheduleBackend for GrpcScheduleBackend {
    fn create_job(
        &self,
        plugin_id: PluginId,
        schedule: &str,
        payload: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
        let client = Arc::clone(&self.client);
        let schedule = schedule.to_owned();
        Box::pin(async move {
            let lock = client.read().await;
            let mut sched = lock
                .clone()
                .ok_or_else(|| "scheduler not connected".to_owned())?;

            let payload_bytes = serde_json::to_vec(&payload)
                .map_err(|e| format!("failed to serialize payload: {e}"))?;

            let request = tonic::Request::new(scheduler_proto::CreateJobRequest {
                name: format!("plugin-{plugin_id}"),
                owner_type: "plugin".to_owned(),
                owner_id: Some(plugin_id.to_string()),
                schedule,
                payload: payload_bytes,
                workspace_id: String::new(),
                created_by: String::new(),
                conversation_id: String::new(),
            });

            let response = sched
                .create_job(request)
                .await
                .map_err(|e| format!("scheduler create_job failed: {e}"))?;

            Ok(response.into_inner().id)
        })
    }
}
