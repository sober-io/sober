//! Conversion between proto and domain [`ContentBlock`] types.

use sober_core::types::ContentBlock;
use sober_core::types::ids::ConversationAttachmentId;

use super::proto;

/// Converts domain content blocks to their proto representation.
pub(crate) fn domain_to_proto(blocks: &[ContentBlock]) -> Vec<proto::ContentBlock> {
    blocks
        .iter()
        .map(|block| {
            let b = match block {
                ContentBlock::Text { text } => {
                    proto::content_block::Block::Text(proto::TextBlock { text: text.clone() })
                }
                ContentBlock::Image {
                    conversation_attachment_id,
                    alt,
                } => proto::content_block::Block::Image(proto::ImageBlock {
                    conversation_attachment_id: conversation_attachment_id.to_string(),
                    alt: alt.clone(),
                }),
                ContentBlock::File {
                    conversation_attachment_id,
                } => proto::content_block::Block::File(proto::FileBlock {
                    conversation_attachment_id: conversation_attachment_id.to_string(),
                }),
                ContentBlock::Audio {
                    conversation_attachment_id,
                } => proto::content_block::Block::Audio(proto::AudioBlock {
                    conversation_attachment_id: conversation_attachment_id.to_string(),
                }),
                ContentBlock::Video {
                    conversation_attachment_id,
                } => proto::content_block::Block::Video(proto::VideoBlock {
                    conversation_attachment_id: conversation_attachment_id.to_string(),
                }),
            };
            proto::ContentBlock { block: Some(b) }
        })
        .collect()
}

/// Converts proto content blocks to their domain representation.
///
/// Blocks with missing `oneof` or unparseable attachment IDs are silently
/// filtered out.
pub(crate) fn proto_to_domain(blocks: &[proto::ContentBlock]) -> Vec<ContentBlock> {
    blocks
        .iter()
        .filter_map(|block| {
            block.block.as_ref().map(|b| match b {
                proto::content_block::Block::Text(t) => ContentBlock::Text {
                    text: t.text.clone(),
                },
                proto::content_block::Block::Image(i) => ContentBlock::Image {
                    conversation_attachment_id: ConversationAttachmentId::from_uuid(
                        uuid::Uuid::parse_str(&i.conversation_attachment_id).unwrap_or_default(),
                    ),
                    alt: i.alt.clone(),
                },
                proto::content_block::Block::File(f) => ContentBlock::File {
                    conversation_attachment_id: ConversationAttachmentId::from_uuid(
                        uuid::Uuid::parse_str(&f.conversation_attachment_id).unwrap_or_default(),
                    ),
                },
                proto::content_block::Block::Audio(a) => ContentBlock::Audio {
                    conversation_attachment_id: ConversationAttachmentId::from_uuid(
                        uuid::Uuid::parse_str(&a.conversation_attachment_id).unwrap_or_default(),
                    ),
                },
                proto::content_block::Block::Video(v) => ContentBlock::Video {
                    conversation_attachment_id: ConversationAttachmentId::from_uuid(
                        uuid::Uuid::parse_str(&v.conversation_attachment_id).unwrap_or_default(),
                    ),
                },
            })
        })
        .collect()
}
