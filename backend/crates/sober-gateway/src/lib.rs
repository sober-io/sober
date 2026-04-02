//! Sober Gateway — bridges external messaging platforms to Sõber conversations.

pub mod bridge;
pub mod discord;
pub mod error;
pub mod grpc;
pub mod outbound;
pub mod service;
pub mod types;

pub mod proto {
    tonic::include_proto!("sober.gateway.v1");
}

pub mod agent_proto {
    tonic::include_proto!("sober.agent.v1");
}
