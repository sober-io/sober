//! Attachment upload and serving endpoints.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, ConversationAttachment, ConversationAttachmentId, ConversationAttachmentRepo,
    ConversationId,
};
use sober_db::PgConversationAttachmentRepo;

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
async fn upload_attachment(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<ConversationId>,
    mut multipart: Multipart,
) -> Result<ApiResponse<ConversationAttachment>, AppError> {
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

    let attachment = state
        .attachment
        .upload(id, auth_user.user_id, filename, data)
        .await?;

    Ok(ApiResponse::new(attachment))
}

/// `GET /api/v1/attachments/:id/content` — serve attachment content.
async fn serve_attachment(
    State(state): State<Arc<AppState>>,
    Path(id): Path<ConversationAttachmentId>,
) -> Result<Response, AppError> {
    let repo = PgConversationAttachmentRepo::new(state.db.clone());
    let attachment = repo.get_by_id(id).await?;

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
