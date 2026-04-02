use std::sync::Arc;
use std::time::Instant;

use metrics::{counter, histogram};
use sober_core::error::AppError;
use sober_core::types::{
    AttachmentKind, ConversationAttachment, ConversationAttachmentRepo, ConversationId,
    CreateConversationAttachment, UserId,
};
use sober_db::PgConversationAttachmentRepo;
use sober_workspace::BlobStore;
use sober_workspace::image_processing;
use sober_workspace::text_extraction;
use sqlx::PgPool;
use tracing::instrument;

pub struct AttachmentService {
    db: PgPool,
    blob_store: Arc<BlobStore>,
}

impl AttachmentService {
    pub fn new(db: PgPool, blob_store: Arc<BlobStore>) -> Self {
        Self { db, blob_store }
    }

    /// Process and store an uploaded file attachment.
    #[instrument(skip(self, data), fields(conversation.id = %conversation_id, attachment.filename = %filename))]
    pub async fn upload(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        filename: String,
        data: Vec<u8>,
    ) -> Result<ConversationAttachment, AppError> {
        let start = Instant::now();

        super::verify_membership(&self.db, conversation_id, user_id).await?;

        // Validate content type via magic bytes.
        let content_type = image_processing::validate_content_type(&data)
            .ok_or_else(|| AppError::Validation("unsupported or unrecognised file type".into()))?;

        let kind = image_processing::derive_attachment_kind(content_type);

        // Process the file based on kind.
        let (store_data, final_content_type, metadata) = match kind {
            AttachmentKind::Image => {
                let processed = image_processing::process_image(&data, content_type)
                    .map_err(|e| AppError::Internal(e.into()))?;
                let metadata = serde_json::json!({
                    "width": processed.width,
                    "height": processed.height,
                });
                (processed.data, processed.content_type, metadata)
            }
            AttachmentKind::Document => {
                let extracted = text_extraction::extract_text(&data, content_type)
                    .map_err(|e| AppError::Internal(e.into()))?;
                let metadata = match extracted {
                    Some(text) => serde_json::json!({ "extracted_text": text }),
                    None => serde_json::json!({}),
                };
                (data, content_type.to_string(), metadata)
            }
            _ => (data, content_type.to_string(), serde_json::json!({})),
        };

        // Store blob.
        let blob_key = self
            .blob_store
            .store(&store_data)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        // Create attachment record.
        let repo = PgConversationAttachmentRepo::new(self.db.clone());
        let attachment = repo
            .create(CreateConversationAttachment {
                blob_key,
                kind,
                content_type: final_content_type,
                filename,
                size: store_data.len() as i64,
                metadata,
                conversation_id,
                user_id,
            })
            .await?;

        let kind_label = match kind {
            AttachmentKind::Image => "image",
            AttachmentKind::Audio => "audio",
            AttachmentKind::Video => "video",
            AttachmentKind::Document => "document",
        };
        counter!("sober_attachment_uploads_total", "kind" => kind_label, "status" => "success")
            .increment(1);
        histogram!("sober_attachment_upload_bytes").record(store_data.len() as f64);
        histogram!("sober_attachment_upload_duration_seconds")
            .record(start.elapsed().as_secs_f64());

        Ok(attachment)
    }
}
