//! Garbage collection commands.

use anyhow::{Context, Result};

use super::scheduler::{connect, proto};
use crate::cli::GcCommand;

/// System job name for blob GC. Must match
/// `sober_scheduler::executors::blob_gc::JOB_NAME`.
const BLOB_GC_JOB_NAME: &str = "system::blob_gc";

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
        .list_jobs(proto::ListJobsRequest {
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
        .find(|j| j.name == BLOB_GC_JOB_NAME)
        .ok_or_else(|| anyhow::anyhow!("system::blob_gc job not found — is blob GC registered?"))?;

    let resp = client
        .force_run(proto::ForceRunRequest {
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
