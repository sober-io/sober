//! Shared utilities for CLI commands that communicate with the agent gRPC service.

use anyhow::{Context, Result};
use hyper_util::rt::TokioIo;
use tonic::transport::{Endpoint, Uri};
use tower::service_fn;

/// Generated protobuf types for the agent gRPC service.
pub(super) mod proto {
    tonic::include_proto!("sober.agent.v1");
}

pub(super) use proto::agent_service_client::AgentServiceClient;

/// Connect to the agent gRPC service over a Unix domain socket.
pub(super) async fn connect_agent(
    socket_path: &str,
) -> Result<AgentServiceClient<tonic::transport::Channel>> {
    let path = std::path::PathBuf::from(socket_path);
    if !path.exists() {
        anyhow::bail!(
            "agent socket not found at {} — is sober-agent running?",
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
        .with_context(|| format!("failed to connect to agent at {socket_path}"))?;

    Ok(AgentServiceClient::new(channel))
}
