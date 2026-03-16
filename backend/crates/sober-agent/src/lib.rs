//! Sober Agent — gRPC server for agent orchestration.

pub mod agent;
pub mod broadcast;
pub mod confirm;
pub mod error;
pub mod extraction;
pub mod grpc;
pub mod stream;
pub mod system_jobs;
pub mod tools;

pub use broadcast::{ConversationUpdateReceiver, ConversationUpdateSender};
pub use confirm::{ConfirmationBroker, ConfirmationRegistrar, ConfirmationSender};
pub use error::AgentError;
pub use stream::{AgentEvent, AgentResponseStream, Usage};

/// Shared scheduler gRPC client handle.
///
/// `None` when the scheduler is not yet connected. The agent can use this to
/// create jobs, list jobs, cancel jobs, etc.
pub type SharedSchedulerClient = std::sync::Arc<
    tokio::sync::RwLock<
        Option<
            grpc::scheduler_proto::scheduler_service_client::SchedulerServiceClient<
                tonic::transport::Channel,
            >,
        >,
    >,
>;
