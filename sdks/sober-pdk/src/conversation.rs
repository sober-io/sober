//! Read access to conversation context.
//!
//! Requires the `conversation_read` capability in `plugin.toml`.
//!
//! # Example
//!
//! ```rust,ignore
//! use sober_pdk::conversation;
//!
//! let messages = conversation::read("conv-id-here", Some(10))?;
//! ```

use serde::{Deserialize, Serialize};

#[extism_pdk::host_fn("sober")]
extern "ExtismHost" {
    fn host_conversation_read(input: String) -> String;
}

#[derive(Serialize)]
struct ConversationReadRequest {
    conversation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Deserialize)]
struct ConversationReadResponse {
    messages: Vec<ConversationMessage>,
}

fn check_error(response: &str) -> Result<(), extism_pdk::Error> {
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(response)
        && let Some(err) = obj.get("error").and_then(|e| e.as_str())
    {
        return Err(extism_pdk::Error::msg(err.to_string()));
    }
    Ok(())
}

/// Reads recent messages from a conversation.
pub fn read(
    conversation_id: &str,
    limit: Option<u32>,
) -> Result<Vec<ConversationMessage>, extism_pdk::Error> {
    let req = serde_json::to_string(&ConversationReadRequest {
        conversation_id: conversation_id.to_string(),
        limit,
    })?;

    let resp = unsafe { host_conversation_read(req)? };
    check_error(&resp)?;

    let parsed: ConversationReadResponse = serde_json::from_str(&resp)?;
    Ok(parsed.messages)
}
