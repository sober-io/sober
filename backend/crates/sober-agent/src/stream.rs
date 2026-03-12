//! Agent event streaming types.

use std::pin::Pin;

use futures::Stream;
use serde::{Deserialize, Serialize};
use sober_core::MessageId;

use crate::error::AgentError;

/// Token usage statistics from an LLM completion.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Usage {
    /// Number of tokens in the prompt.
    pub prompt_tokens: u32,
    /// Number of tokens in the completion.
    pub completion_tokens: u32,
}

/// Events emitted by the agent during message handling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentEvent {
    /// Incremental text output from the LLM.
    TextDelta(String),
    /// A tool call has started.
    ToolCallStart {
        /// Name of the tool being called.
        name: String,
        /// JSON-encoded input for the tool.
        input: serde_json::Value,
    },
    /// A tool call has completed.
    ToolCallResult {
        /// Name of the tool that was called.
        name: String,
        /// Output produced by the tool.
        output: String,
    },
    /// The agent has finished processing.
    Done {
        /// Unique identifier for the resulting message.
        message_id: MessageId,
        /// Token usage statistics.
        usage: Usage,
    },
    /// An auto-generated title for the conversation.
    TitleGenerated(String),
    /// A confirmation request for user approval of a shell command.
    ConfirmRequest {
        /// Unique ID for this confirmation request.
        confirm_id: String,
        /// The command that needs approval.
        command: String,
        /// Risk level: "Safe", "Moderate", or "Dangerous".
        risk_level: String,
        /// Resources affected by this command.
        affects: Vec<String>,
        /// Human-readable reason for requiring confirmation.
        reason: String,
    },
    /// An error occurred during processing.
    Error(String),
}

/// A pinned, boxed stream of agent events.
pub type AgentResponseStream = Pin<Box<dyn Stream<Item = Result<AgentEvent, AgentError>> + Send>>;
