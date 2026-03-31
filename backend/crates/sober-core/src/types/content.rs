//! Content block types for multimodal message content.

use serde::{Deserialize, Serialize};

use super::ids::ConversationAttachmentId;

/// A single content block within a message.
///
/// Messages contain `Vec<ContentBlock>` instead of a plain `String`.
/// Tagged serde serialization: `{"type": "text", "text": "..."}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text content.
    Text {
        /// The text content.
        text: String,
    },
    /// An image attachment.
    Image {
        /// Reference to the uploaded attachment.
        conversation_attachment_id: ConversationAttachmentId,
        /// Optional alt text for accessibility.
        #[serde(skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
    },
    /// A file attachment (PDF, text, etc.).
    File {
        /// Reference to the uploaded attachment.
        conversation_attachment_id: ConversationAttachmentId,
    },
    /// An audio attachment.
    Audio {
        /// Reference to the uploaded attachment.
        conversation_attachment_id: ConversationAttachmentId,
    },
    /// A video attachment.
    Video {
        /// Reference to the uploaded attachment.
        conversation_attachment_id: ConversationAttachmentId,
    },
}

impl ContentBlock {
    /// Creates a text content block.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_block_serde_roundtrip() {
        let block = ContentBlock::text("hello");
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["text"], "hello");

        let deserialized: ContentBlock = serde_json::from_value(json).unwrap();
        match deserialized {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn image_block_serde_roundtrip() {
        let id = ConversationAttachmentId::new();
        let block = ContentBlock::Image {
            conversation_attachment_id: id,
            alt: Some("a photo".into()),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "image");
        assert!(json["alt"].is_string());

        let deserialized: ContentBlock = serde_json::from_value(json).unwrap();
        match deserialized {
            ContentBlock::Image {
                conversation_attachment_id,
                alt,
            } => {
                assert_eq!(conversation_attachment_id, id);
                assert_eq!(alt.as_deref(), Some("a photo"));
            }
            _ => panic!("expected Image variant"),
        }
    }

    #[test]
    fn image_block_without_alt_skips_field() {
        let block = ContentBlock::Image {
            conversation_attachment_id: ConversationAttachmentId::new(),
            alt: None,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(!json.contains("alt"));
    }

    #[test]
    fn file_block_serde_roundtrip() {
        let id = ConversationAttachmentId::new();
        let block = ContentBlock::File {
            conversation_attachment_id: id,
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "file");
        let deserialized: ContentBlock = serde_json::from_value(json).unwrap();
        match deserialized {
            ContentBlock::File {
                conversation_attachment_id,
            } => {
                assert_eq!(conversation_attachment_id, id);
            }
            _ => panic!("expected File variant"),
        }
    }
}
