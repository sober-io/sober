//! Sober API — HTTP/WebSocket gateway library.
//!
//! Shared modules used by the `sober-api` binary and integration tests.

pub mod admin;
pub mod middleware;
pub mod routes;
pub mod state;

/// Generated protobuf types for the agent gRPC service.
pub mod proto {
    tonic::include_proto!("sober.agent.v1");
}
