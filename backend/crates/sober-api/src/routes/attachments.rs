//! Attachment upload and serving endpoints.

use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use metrics::{counter, histogram};
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, AttachmentKind, ConversationAttachment, ConversationAttachmentId,
    ConversationAttachmentRepo, ConversationId, CreateConversationAttachment,
};
use sober_db::PgConversationAttachmentRepo;
use sober_workspace::image_processing;
use sober_workspace::text_extraction;

use crate::state::AppState;

/// Maximum upload size: 25 MB.
const MAX_UPLOAD_SIZE: usize = 25 * 1024 * 1024;

/// Returns the attachment routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/conversations/{id}/attachments", post(upload_attachment))
        .route("/attachments/{id}/content", get(serve_attachment))
}

/// `POST /api/v1/conversations/:id/attachments` — upload a file attachment.
///
/// Accepts `multipart/form-data` with a `file` field. Validates content type
/// via magic bytes, processes images (resize), extracts text from documents,
/// stores the blob, and creates the attachment record.
async fn upload_attachment(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<ConversationId>,
    mut multipart: Multipart,
) -> Result<ApiResponse<ConversationAttachment>, AppError> {
    let start = Instant::now();

    // Verify membership.
    let _membership = super::verify_membership(&state.db, id, auth_user.user_id).await?;

    // Extract the file field from multipart.
    let mut file_data: Option<(String, Vec<u8>)> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Validation(format!("multipart error: {e}")))?
    {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            let filename = field.file_name().unwrap_or("upload").to_string();

            let data = field
                .bytes()
                .await
                .map_err(|e| AppError::Validation(format!("failed to read file: {e}")))?;

            if data.len() > MAX_UPLOAD_SIZE {
                return Err(AppError::Validation(format!(
                    "file exceeds maximum size of {} MB",
                    MAX_UPLOAD_SIZE / (1024 * 1024)
                )));
            }

            file_data = Some((filename, data.to_vec()));
            break;
        }
    }

    let (filename, data) = file_data
        .ok_or_else(|| AppError::Validation("missing 'file' field in multipart upload".into()))?;

    // Validate content type via magic bytes.
    let content_type = image_processing::validate_content_type(&data)
        .ok_or_else(|| AppError::Validation("unsupported or unrecognised file type".into()))?;

    // Derive attachment kind from content type.
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
    let blob_key = state
        .blob_store
        .store(&store_data)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    // Create attachment record.
    let repo = PgConversationAttachmentRepo::new(state.db.clone());
    let attachment = repo
        .create(CreateConversationAttachment {
            blob_key,
            kind,
            content_type: final_content_type,
            filename,
            size: store_data.len() as i64,
            metadata,
            conversation_id: id,
            user_id: auth_user.user_id,
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

    Ok(ApiResponse::new(attachment))
}

/// `GET /api/v1/attachments/:id/content` — serve attachment content.
///
/// Returns the raw file bytes with appropriate content type and caching headers.
async fn serve_attachment(
    State(state): State<Arc<AppState>>,
    Path(id): Path<ConversationAttachmentId>,
) -> Result<Response, AppError> {
    let repo = PgConversationAttachmentRepo::new(state.db.clone());
    let attachment = repo.get_by_id(id).await?;

    // Read blob data.
    let data = state
        .blob_store
        .retrieve(&attachment.blob_key)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let disposition = format!("attachment; filename=\"{}\"", attachment.filename);

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, &attachment.content_type)
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .body(Body::from(data))
        .map_err(|e| AppError::Internal(e.into()))?;

    Ok(response)
}
