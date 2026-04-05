use std::sync::Arc;

use sober_core::error::AppError;
use sober_core::types::{ConversationAttachment, ConversationId, UserId};
use sober_workspace::BlobStore;
use sqlx::PgPool;

pub struct AttachmentService {
    db: PgPool,
    blob_store: Arc<BlobStore>,
}

impl AttachmentService {
    pub fn new(db: PgPool, blob_store: Arc<BlobStore>) -> Self {
        Self { db, blob_store }
    }

    /// Process and store an uploaded file attachment.
    pub async fn upload(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        filename: String,
        data: Vec<u8>,
    ) -> Result<ConversationAttachment, AppError> {
        super::verify_membership(&self.db, conversation_id, user_id).await?;

        sober_workspace::attachment::process_and_store_attachment(
            &self.db,
            &self.blob_store,
            conversation_id,
            user_id,
            filename,
            data,
        )
        .await
    }
}
