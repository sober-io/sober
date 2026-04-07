//! WebSocket message types shared across modules.

use sober_core::types::{ContentBlock, MessageSource};

/// Basic user info included in collaborator change WebSocket events.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct CollaboratorInfo {
    /// User ID.
    pub id: String,
    /// Username.
    pub username: String,
}

/// Server-to-client WebSocket message types.
#[derive(serde::Serialize, Clone)]
#[serde(tag = "type")]
pub enum ServerWsMessage {
    /// Incremental text from the assistant.
    #[serde(rename = "chat.delta")]
    ChatDelta {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// Text fragment.
        content: String,
    },
    /// A tool execution status update.
    #[serde(rename = "chat.tool_execution_update")]
    ChatToolExecutionUpdate {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// Tool execution UUID.
        id: String,
        /// Assistant message UUID.
        message_id: String,
        /// LLM-assigned tool call ID.
        tool_call_id: String,
        /// Name of the tool.
        tool_name: String,
        /// Execution status: pending, running, completed, failed, cancelled.
        status: String,
        /// Tool output (when completed).
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        /// Error message (when failed).
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        /// JSON-encoded tool arguments (sent on pending event).
        #[serde(skip_serializing_if = "Option::is_none")]
        input: Option<String>,
    },
    /// The agent has finished processing.
    #[serde(rename = "chat.done")]
    ChatDone {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// ID of the stored assistant message.
        message_id: String,
        /// Final cleaned content (extraction blocks stripped). Empty if unchanged.
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
    },
    /// Thinking/reasoning content from the model.
    #[serde(rename = "chat.thinking")]
    ChatThinking {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// Thinking text fragment.
        content: String,
    },
    /// Agent is processing a message (typing indicator for other group members).
    #[serde(rename = "chat.agent_typing")]
    ChatAgentTyping {
        /// Conversation this event belongs to.
        conversation_id: String,
    },
    /// The conversation title was generated or changed.
    #[serde(rename = "chat.title")]
    ChatTitle {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// The new title.
        title: String,
    },
    /// An error occurred.
    #[serde(rename = "chat.error")]
    ChatError {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// Error description.
        error: String,
    },
    /// A shell command confirmation request.
    #[serde(rename = "chat.confirm")]
    ChatConfirm {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// Unique ID for this confirmation request.
        confirm_id: String,
        /// The command that needs approval.
        command: String,
        /// Risk level assessment.
        risk_level: String,
        /// Resources affected.
        affects: Vec<String>,
        /// Reason for requiring confirmation.
        reason: String,
    },
    /// A new message was stored in the conversation.
    #[serde(rename = "chat.new_message")]
    ChatNewMessage {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// ID of the stored message.
        message_id: String,
        /// Role of the message author.
        role: String,
        /// Message content blocks.
        content: Vec<ContentBlock>,
        /// What produced this message.
        source: MessageSource,
        /// User ID of the sender (if applicable).
        #[serde(skip_serializing_if = "Option::is_none")]
        user_id: Option<String>,
        /// Username of the sender (if applicable).
        #[serde(skip_serializing_if = "Option::is_none")]
        username: Option<String>,
    },
    /// A message's content was updated (e.g., secret redaction).
    #[serde(rename = "chat.message_updated")]
    ChatMessageUpdated {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// ID of the updated message.
        message_id: String,
        /// Updated content blocks (JSON-encoded).
        content: String,
        /// Redacted reasoning/thinking content (if present).
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning: Option<String>,
    },
    /// Unread count changed for a conversation.
    #[serde(rename = "chat.unread")]
    ChatUnread {
        /// Conversation with unread messages.
        conversation_id: String,
        /// New unread count.
        unread_count: i32,
    },
    /// A collaborator was added to the conversation.
    #[serde(rename = "chat.collaborator_added")]
    ChatCollaboratorAdded {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// The added user.
        user: CollaboratorInfo,
        /// The role assigned.
        role: String,
    },
    /// A collaborator was removed from the conversation.
    #[serde(rename = "chat.collaborator_removed")]
    ChatCollaboratorRemoved {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// The removed user's ID.
        user_id: String,
    },
    /// A collaborator's role was changed.
    #[serde(rename = "chat.role_changed")]
    ChatRoleChanged {
        /// Conversation this event belongs to.
        conversation_id: String,
        /// The user whose role changed.
        user_id: String,
        /// The new role.
        role: String,
    },
    /// Keepalive response.
    #[serde(rename = "pong")]
    Pong,
}
