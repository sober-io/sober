//! Sober Scheduler — autonomous tick engine with interval and cron scheduling,
//! job persistence, and gRPC admin interface.

pub mod engine;
pub mod error;
pub mod grpc;
pub mod job;
