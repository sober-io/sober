//! Sober Agent — gRPC server for agent orchestration.

pub mod agent;
pub mod error;
pub mod grpc;
pub mod stream;
pub mod tools;

pub use error::AgentError;
pub use stream::{AgentEvent, AgentResponseStream, Usage};
