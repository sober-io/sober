//! Sober API — HTTP/WebSocket gateway library.
//!
//! Shared modules used by the `sober-api` binary and integration tests.

pub mod admin;
pub mod connections;
pub mod guards;
pub mod middleware;
pub mod routes;
pub mod services;
pub mod state;
pub mod subscribe;
pub mod ws_types;

/// Generated protobuf types for the agent gRPC service.
pub mod proto {
    tonic::include_proto!("sober.agent.v1");
}

/// Generated protobuf types for the gateway gRPC service.
pub mod gateway_proto {
    tonic::include_proto!("sober.gateway.v1");
}
