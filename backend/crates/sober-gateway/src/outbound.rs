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
    /// Internal metadata tags (e.g. `<memory_extractions>`) are stripped before
    /// returning. Returns `None` if the buffer is empty after stripping.
    pub fn flush(&mut self, conversation_id: &ConversationId) -> Option<PlatformMessage> {
        self.buffers.remove(conversation_id).and_then(|text| {
            let cleaned = strip_metadata_tags(&text);
            if cleaned.is_empty() {
                None
            } else {
                Some(PlatformMessage {
                    text: cleaned,
                    format: MessageFormat::Markdown,
                    reply_to: None,
                })
            }
        })
    }
}

/// Strips internal metadata XML-style tags from agent responses.
///
/// Removes blocks matching `<tag_name>...</tag_name>` where `tag_name` uses
/// snake_case (letters, digits, underscores). This catches current tags like
/// `<memory_extractions>` and any future internal metadata the agent appends.
///
/// The resulting text is trimmed of leading/trailing whitespace.
fn strip_metadata_tags(text: &str) -> String {
    let mut result = text.to_owned();
    // Repeatedly strip `<snake_case_tag>...</snake_case_tag>` blocks.
    // Use a simple loop since regex isn't a dependency.
    loop {
        let Some(open_start) = result.find('<') else {
            break;
        };
        let after_open = &result[open_start + 1..];
        let Some(open_end) = after_open.find('>') else {
            break;
        };
        let tag_name = &after_open[..open_end];

        // Only strip snake_case tags (internal metadata), not markdown/HTML.
        if tag_name.is_empty()
            || !tag_name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            break;
        }

        let close_tag = format!("</{tag_name}>");
        let Some(close_pos) = result.find(&close_tag) else {
            break;
        };

        let before = result[..open_start].trim_end();
        let after = result[close_pos + close_tag.len()..].trim_start();
        result = if before.is_empty() || after.is_empty() {
            format!("{before}{after}")
        } else {
            format!("{before}\n\n{after}")
        };
    }
    result.trim().to_owned()
}

/// Splits text into chunks that fit within `max_len` characters.
///
/// Split priority (best to worst):
/// 1. Last paragraph break (`\n\n`) — keeps markdown blocks intact
/// 2. Last newline (`\n`) — keeps lines intact
/// 3. Last sentence boundary (`. `, `! `, `? `) — keeps sentences intact
/// 4. Last word boundary (` `) — avoids splitting words
/// 5. Hard split at limit — last resort
///
/// Platform-specific limits: Discord: 2000, Telegram: 4096, Matrix/WhatsApp: ~65535.
pub fn split_message(text: &str, max_len: usize) -> Vec<&str> {
    if text.len() <= max_len {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining);
            break;
        }

        let window = &remaining[..max_len];
        let split_at = window
            .rfind("\n\n")
            .map(|p| p + 1)
            .or_else(|| window.rfind('\n').map(|p| p + 1))
            .or_else(|| {
                window
                    .rfind(". ")
                    .or_else(|| window.rfind("! "))
                    .or_else(|| window.rfind("? "))
                    .map(|p| p + 2) // include the punctuation + space
            })
            .or_else(|| window.rfind(' ').map(|p| p + 1))
            .unwrap_or(max_len);

        chunks.push(&remaining[..split_at]);
        remaining = &remaining[split_at..];
    }

    chunks
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

    #[test]
    fn strips_memory_extractions() {
        assert_eq!(
            strip_metadata_tags("Hello!\n\n<memory_extractions>\n[]\n</memory_extractions>"),
            "Hello!"
        );
    }

    #[test]
    fn strips_multiple_tags() {
        assert_eq!(
            strip_metadata_tags(
                "Response.\n\n<memory_extractions>\n[]\n</memory_extractions>\n<some_future_tag>\ndata\n</some_future_tag>"
            ),
            "Response."
        );
    }

    #[test]
    fn preserves_normal_markdown() {
        assert_eq!(
            strip_metadata_tags("Use `<T>` for generics"),
            "Use `<T>` for generics"
        );
    }

    #[test]
    fn returns_empty_for_only_metadata() {
        let mut buf = OutboundBuffer::new();
        let conv = ConversationId::new();
        buf.append_delta(conv, "<memory_extractions>\n[]\n</memory_extractions>");
        assert!(buf.flush(&conv).is_none());
    }

    #[test]
    fn strips_tag_with_content_before_and_after() {
        assert_eq!(
            strip_metadata_tags("Before\n<internal_tag>\nstuff\n</internal_tag>\nAfter"),
            "Before\n\nAfter"
        );
    }
}
