use std::sync::Arc;

use sober_workspace::BlobStore;
use sqlx::PgPool;

pub struct AttachmentService {
    pub(crate) db: PgPool,
    pub(crate) blob_store: Arc<BlobStore>,
}

impl AttachmentService {
    pub fn new(db: PgPool, blob_store: Arc<BlobStore>) -> Self {
        Self { db, blob_store }
    }
}
