use std::collections::HashMap;

use serde::Serialize;
use sober_core::error::AppError;
use sober_core::types::{
    ContentBlock, ConversationAttachment, ConversationAttachmentId, ConversationAttachmentRepo,
    ConversationId, Message, MessageId, MessageRepo, MessageRole, Tag, TagRepo, ToolExecution,
    ToolExecutionRepo, UserId,
};
use sober_db::{PgConversationAttachmentRepo, PgMessageRepo, PgTagRepo, PgToolExecutionRepo};
use sqlx::PgPool;

use tracing::instrument;

use crate::guards;

/// A message with its associated tags, tool executions, and attachments.
#[derive(Serialize)]
pub struct MessageWithDetails {
    #[serde(flatten)]
    pub message: Message,
    pub tags: Vec<Tag>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_executions: Vec<ToolExecution>,
    pub attachments: HashMap<String, ConversationAttachment>,
}

pub struct MessageService {
    db: PgPool,
}

impl MessageService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// List messages with tags, tool executions, and attachments.
    #[instrument(level = "debug", skip(self), fields(conversation.id = %conversation_id))]
    pub async fn list(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        before: Option<MessageId>,
        limit: i64,
    ) -> Result<Vec<MessageWithDetails>, AppError> {
        super::verify_membership(&self.db, conversation_id, user_id).await?;

        let msg_repo = PgMessageRepo::new(self.db.clone());
        let tag_repo = PgTagRepo::new(self.db.clone());
        let tool_exec_repo = PgToolExecutionRepo::new(self.db.clone());

        let messages = msg_repo
            .list_paginated(conversation_id, before, limit)
            .await?;

        // Batch-fetch tags.
        let msg_ids: Vec<_> = messages.iter().map(|m| m.id).collect();
        let tag_pairs = tag_repo
            .list_by_message_ids(&msg_ids)
            .await
            .unwrap_or_default();
        let mut tag_map: HashMap<MessageId, Vec<Tag>> = HashMap::new();
        for (msg_id, tag) in tag_pairs {
            tag_map.entry(msg_id).or_default().push(tag);
        }

        // Batch-fetch tool executions for assistant messages.
        let mut exec_map: HashMap<MessageId, Vec<ToolExecution>> = HashMap::new();
        for msg in &messages {
            if msg.role == MessageRole::Assistant {
                let execs = tool_exec_repo
                    .find_by_message(msg.id)
                    .await
                    .unwrap_or_default();
                if !execs.is_empty() {
                    exec_map.insert(msg.id, execs);
                }
            }
        }

        // Collect all attachment IDs from content blocks.
        let mut all_attachment_ids: Vec<ConversationAttachmentId> = Vec::new();
        for msg in &messages {
            for block in &msg.content {
                if let Some(id) = attachment_id_from_block(block) {
                    all_attachment_ids.push(id);
                }
            }
        }

        // Batch-fetch attachment metadata.
        let mut attachment_map: HashMap<String, ConversationAttachment> = HashMap::new();
        if !all_attachment_ids.is_empty() {
            all_attachment_ids.dedup();
            let attachment_repo = PgConversationAttachmentRepo::new(self.db.clone());
            let attachments = attachment_repo
                .get_by_ids(&all_attachment_ids)
                .await
                .unwrap_or_default();
            for att in attachments {
                attachment_map.insert(att.id.to_string(), att);
            }
        }

        // Assemble response.
        let result = messages
            .into_iter()
            .map(|m| {
                let tags = tag_map.remove(&m.id).unwrap_or_default();
                let tool_executions = exec_map.remove(&m.id).unwrap_or_default();

                // Build per-message attachment map.
                let mut msg_attachments = HashMap::new();
                for block in &m.content {
                    if let Some(id) = attachment_id_from_block(block) {
                        let id_str = id.to_string();
                        if let Some(att) = attachment_map.get(&id_str) {
                            msg_attachments.insert(id_str, att.clone());
                        }
                    }
                }

                MessageWithDetails {
                    message: m,
                    tags,
                    tool_executions,
                    attachments: msg_attachments,
                }
            })
            .collect();

        Ok(result)
    }

    /// Delete a message and clean up orphaned attachments.
    #[instrument(skip(self), fields(message.id = %message_id))]
    pub async fn delete(&self, message_id: MessageId, user_id: UserId) -> Result<(), AppError> {
        let msg_repo = PgMessageRepo::new(self.db.clone());
        let msg = msg_repo.get_by_id(message_id).await?;

        let membership = super::verify_membership(&self.db, msg.conversation_id, user_id).await?;

        guards::require_owner_or_sender(&membership, msg.user_id, user_id)?;

        let attachment_ids: Vec<ConversationAttachmentId> = msg
            .content
            .iter()
            .filter_map(attachment_id_from_block)
            .collect();

        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        PgMessageRepo::delete_tx(&mut tx, message_id).await?;

        if !attachment_ids.is_empty() {
            let orphaned = PgConversationAttachmentRepo::find_unreferenced_by_message_tx(
                &mut tx,
                &attachment_ids,
                msg.conversation_id,
            )
            .await?;
            for orphan_id in orphaned {
                PgConversationAttachmentRepo::delete_tx(&mut tx, orphan_id).await?;
            }
        }

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }
}

fn attachment_id_from_block(block: &ContentBlock) -> Option<ConversationAttachmentId> {
    match block {
        ContentBlock::Image {
            conversation_attachment_id,
            ..
        }
        | ContentBlock::File {
            conversation_attachment_id,
        }
        | ContentBlock::Audio {
            conversation_attachment_id,
        }
        | ContentBlock::Video {
            conversation_attachment_id,
        } => Some(*conversation_attachment_id),
        ContentBlock::Text { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_image_attachment_id() {
        let id = ConversationAttachmentId::new();
        let block = ContentBlock::Image {
            conversation_attachment_id: id,
            alt: Some("test".into()),
        };
        assert_eq!(attachment_id_from_block(&block), Some(id));
    }

    #[test]
    fn extracts_file_attachment_id() {
        let id = ConversationAttachmentId::new();
        let block = ContentBlock::File {
            conversation_attachment_id: id,
        };
        assert_eq!(attachment_id_from_block(&block), Some(id));
    }

    #[test]
    fn extracts_audio_attachment_id() {
        let id = ConversationAttachmentId::new();
        let block = ContentBlock::Audio {
            conversation_attachment_id: id,
        };
        assert_eq!(attachment_id_from_block(&block), Some(id));
    }

    #[test]
    fn extracts_video_attachment_id() {
        let id = ConversationAttachmentId::new();
        let block = ContentBlock::Video {
            conversation_attachment_id: id,
        };
        assert_eq!(attachment_id_from_block(&block), Some(id));
    }

    #[test]
    fn text_block_returns_none() {
        let block = ContentBlock::Text {
            text: "hello".into(),
        };
        assert_eq!(attachment_id_from_block(&block), None);
    }
}
