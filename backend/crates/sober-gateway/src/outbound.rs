//! Outbound message buffering for streaming agent responses.

use std::collections::HashMap;

use sober_core::types::ConversationId;

use crate::types::{MessageFormat, PlatformMessage};

/// Accumulates text deltas for in-flight agent responses.
///
/// One entry per conversation — deltas are appended until the agent signals
/// `Done`, at which point `flush` drains the buffer and returns the assembled
/// message for delivery.
pub struct OutboundBuffer {
    buffers: HashMap<ConversationId, String>,
}

impl OutboundBuffer {
    /// Creates a new empty buffer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
        }
    }

    /// Appends a text delta to the buffer for the given conversation.
    pub fn append_delta(&mut self, conversation_id: ConversationId, delta: &str) {
        self.buffers
            .entry(conversation_id)
            .or_default()
            .push_str(delta);
    }

    /// Flushes the buffer for a conversation and returns the assembled message.
    ///
    /// Returns `None` if the buffer is empty or missing.
    pub fn flush(&mut self, conversation_id: &ConversationId) -> Option<PlatformMessage> {
        self.buffers.remove(conversation_id).and_then(|text| {
            if text.is_empty() {
                None
            } else {
                Some(PlatformMessage {
                    text,
                    format: MessageFormat::Markdown,
                    reply_to: None,
                })
            }
        })
    }
}

impl Default for OutboundBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_accumulates_and_flushes() {
        let mut buf = OutboundBuffer::new();
        let conv = ConversationId::new();
        buf.append_delta(conv, "Hello ");
        buf.append_delta(conv, "world!");
        let msg = buf.flush(&conv).unwrap();
        assert_eq!(msg.text, "Hello world!");
    }

    #[test]
    fn flush_empty_returns_none() {
        let mut buf = OutboundBuffer::new();
        assert!(buf.flush(&ConversationId::new()).is_none());
    }

    #[test]
    fn flush_clears_buffer() {
        let mut buf = OutboundBuffer::new();
        let conv = ConversationId::new();
        buf.append_delta(conv, "test");
        buf.flush(&conv);
        assert!(buf.flush(&conv).is_none());
    }
}
