//! Tool execution domain types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::domain::Message;
use super::enums::{ToolExecutionSource, ToolExecutionStatus};
use super::ids::{ConversationId, MessageId, PluginId, ToolExecutionId};

/// A tool execution within a conversation turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecution {
    pub id: ToolExecutionId,
    pub conversation_id: ConversationId,
    pub conversation_message_id: MessageId,
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub source: ToolExecutionSource,
    pub status: ToolExecutionStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub plugin_id: Option<PluginId>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Input for creating a pending tool execution.
#[derive(Debug, Clone)]
pub struct CreateToolExecution {
    pub conversation_id: ConversationId,
    pub conversation_message_id: MessageId,
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub source: ToolExecutionSource,
    pub plugin_id: Option<PluginId>,
}

/// A message with its associated tool executions (for LLM context reconstruction).
#[derive(Debug, Clone)]
pub struct MessageWithExecutions {
    pub message: Message,
    pub tool_executions: Vec<ToolExecution>,
}
