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
    /// Incremental reasoning/thinking content from the LLM.
    ThinkingDelta(String),
    /// A tool execution status update.
    ToolExecutionUpdate {
        /// Unique ID of the tool execution.
        id: String,
        /// The assistant message that triggered this tool call.
        message_id: String,
        /// LLM-assigned tool call ID.
        tool_call_id: String,
        /// Name of the tool.
        tool_name: String,
        /// Current status: pending, running, completed, failed, cancelled.
        status: String,
        /// Tool output (when completed).
        output: Option<String>,
        /// Error message (when failed).
        error: Option<String>,
        /// JSON-encoded tool arguments (sent on first event for this execution).
        input: Option<String>,
    },
    /// The agent has finished processing.
    Done {
        /// Unique identifier for the resulting message.
        message_id: MessageId,
        /// Final cleaned text (extraction blocks stripped).
        content: Option<String>,
        /// Token usage statistics.
        usage: Usage,
        /// Optional reference to a stored artifact (e.g. workspace blob path).
        artifact_ref: Option<String>,
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
