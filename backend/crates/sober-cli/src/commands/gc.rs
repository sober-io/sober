//! Garbage collection commands.

use anyhow::{Context, Result};
use hyper_util::rt::TokioIo;
use tonic::transport::{Endpoint, Uri};
use tower::service_fn;

use crate::cli::GcCommand;

/// Generated protobuf types for the scheduler gRPC service.
mod proto {
    tonic::include_proto!("sober.scheduler.v1");
}

use proto::scheduler_service_client::SchedulerServiceClient;
use proto::{ForceRunRequest, ListJobsRequest};

/// Connect to the scheduler gRPC service over a Unix domain socket.
async fn connect(socket_path: &str) -> Result<SchedulerServiceClient<tonic::transport::Channel>> {
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

/// Execute a GC subcommand.
pub async fn handle(cmd: GcCommand) -> Result<()> {
    match cmd {
        GcCommand::Blobs { socket } => run_blob_gc(&socket).await,
    }
}

/// Trigger blob GC via the scheduler's force_run RPC.
///
/// Looks up the `system::blob_gc` job by listing system jobs, then calls
/// `force_run` on it.
async fn run_blob_gc(socket: &str) -> Result<()> {
    let mut client = connect(socket).await?;

    // Find the blob_gc system job.
    let resp = client
        .list_jobs(ListJobsRequest {
            owner_type: Some("system".to_string()),
            owner_id: None,
            statuses: vec![],
            workspace_id: String::new(),
            name_filter: "blob_gc".into(),
        })
        .await
        .context("failed to list jobs")?;

    let jobs = resp.into_inner().jobs;
    let job = jobs
        .iter()
        .find(|j| j.name == "system::blob_gc")
        .ok_or_else(|| anyhow::anyhow!("system::blob_gc job not found — is blob GC registered?"))?;

    let resp = client
        .force_run(ForceRunRequest {
            job_id: job.id.clone(),
        })
        .await
        .context("failed to trigger blob GC")?;

    if resp.into_inner().accepted {
        println!("blob GC triggered (job {})", job.id);
    } else {
        println!("blob GC rejected (job may already be running)");
    }

    Ok(())
}
