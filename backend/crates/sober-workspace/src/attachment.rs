//! Shared attachment processing and storage pipeline.
//!
//! Used by both `sober-api` (HTTP uploads) and `sober-gateway` (platform uploads).

use std::time::Instant;

use metrics::{counter, histogram};
use sober_core::error::AppError;
use sober_core::types::{
    AttachmentKind, ConversationAttachment, ConversationAttachmentRepo, ConversationId,
    CreateConversationAttachment, UserId,
};
use sober_db::PgConversationAttachmentRepo;
use sqlx::PgPool;
use tracing::instrument;

use crate::BlobStore;
use crate::image_processing;
use crate::text_extraction;

/// Validates, processes, and stores a file attachment.
///
/// Pipeline: validate content type via magic bytes → derive kind → process
/// (resize images / extract document text) → store blob → create DB record.
///
/// Does **not** verify conversation membership — callers must do that if needed.
#[instrument(skip(db, blob_store, data), fields(conversation.id = %conversation_id, attachment.filename = %filename))]
pub async fn process_and_store_attachment(
    db: &PgPool,
    blob_store: &BlobStore,
    conversation_id: ConversationId,
    user_id: UserId,
    filename: String,
    data: Vec<u8>,
) -> Result<ConversationAttachment, AppError> {
    let start = Instant::now();

    let content_type = image_processing::validate_content_type(&data)
        .ok_or_else(|| AppError::Validation("unsupported or unrecognised file type".into()))?;

    let kind = image_processing::derive_attachment_kind(content_type);

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

    let blob_key = blob_store
        .store(&store_data)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let repo = PgConversationAttachmentRepo::new(db.clone());
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
    histogram!("sober_attachment_upload_duration_seconds").record(start.elapsed().as_secs_f64());

    Ok(attachment)
}
