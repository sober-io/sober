//! Conversion helpers between proto content blocks and domain `ContentBlock`.

use sober_core::types::{ContentBlock, ConversationAttachmentId};

use crate::proto;

/// Converts a domain `ContentBlock` to a proto `ContentBlock`.
pub fn content_block_to_proto(block: ContentBlock) -> proto::ContentBlock {
    match block {
        ContentBlock::Text { text } => proto::ContentBlock {
            block: Some(proto::content_block::Block::Text(proto::TextBlock { text })),
        },
        ContentBlock::Image {
            conversation_attachment_id,
            alt,
        } => proto::ContentBlock {
            block: Some(proto::content_block::Block::Image(proto::ImageBlock {
                conversation_attachment_id: conversation_attachment_id.to_string(),
                alt,
            })),
        },
        ContentBlock::File {
            conversation_attachment_id,
        } => proto::ContentBlock {
            block: Some(proto::content_block::Block::File(proto::FileBlock {
                conversation_attachment_id: conversation_attachment_id.to_string(),
            })),
        },
        ContentBlock::Audio {
            conversation_attachment_id,
        } => proto::ContentBlock {
            block: Some(proto::content_block::Block::Audio(proto::AudioBlock {
                conversation_attachment_id: conversation_attachment_id.to_string(),
            })),
        },
        ContentBlock::Video {
            conversation_attachment_id,
        } => proto::ContentBlock {
            block: Some(proto::content_block::Block::Video(proto::VideoBlock {
                conversation_attachment_id: conversation_attachment_id.to_string(),
            })),
        },
    }
}

/// Converts a proto content block variant to a domain `ContentBlock`.
///
/// Returns `None` if the attachment ID is not a valid UUID.
pub fn proto_to_content_block(block: &proto::content_block::Block) -> Option<ContentBlock> {
    match block {
        proto::content_block::Block::Text(t) => Some(ContentBlock::text(&t.text)),
        proto::content_block::Block::Image(img) => {
            let id = img.conversation_attachment_id.parse::<uuid::Uuid>().ok()?;
            Some(ContentBlock::Image {
                conversation_attachment_id: ConversationAttachmentId::from_uuid(id),
                alt: img.alt.clone(),
            })
        }
        proto::content_block::Block::File(f) => {
            let id = f.conversation_attachment_id.parse::<uuid::Uuid>().ok()?;
            Some(ContentBlock::File {
                conversation_attachment_id: ConversationAttachmentId::from_uuid(id),
            })
        }
        proto::content_block::Block::Audio(a) => {
            let id = a.conversation_attachment_id.parse::<uuid::Uuid>().ok()?;
            Some(ContentBlock::Audio {
                conversation_attachment_id: ConversationAttachmentId::from_uuid(id),
            })
        }
        proto::content_block::Block::Video(v) => {
            let id = v.conversation_attachment_id.parse::<uuid::Uuid>().ok()?;
            Some(ContentBlock::Video {
                conversation_attachment_id: ConversationAttachmentId::from_uuid(id),
            })
        }
    }
}
