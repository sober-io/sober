use sober_core::error::AppError;
use thiserror::Error;

/// Gateway-specific errors.
#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("platform connection failed: {0}")]
    ConnectionFailed(String),

    #[error("platform send failed: {0}")]
    SendFailed(String),

    #[error("platform not found: {0}")]
    PlatformNotFound(String),

    #[error("channel not found: {0}")]
    ChannelNotFound(String),

    #[error("unmapped channel: platform={platform_id}, channel={channel_id}")]
    UnmappedChannel {
        platform_id: String,
        channel_id: String,
    },

    #[error("attachment download failed: {0}")]
    AttachmentDownloadFailed(String),

    #[error("attachment store failed: {0}")]
    AttachmentStoreFailed(String),

    #[error("attachment fetch failed: {0}")]
    AttachmentFetchFailed(String),
}

impl From<GatewayError> for AppError {
    fn from(err: GatewayError) -> Self {
        match err {
            GatewayError::PlatformNotFound(_) | GatewayError::ChannelNotFound(_) => {
                AppError::NotFound(err.to_string())
            }
            _ => AppError::Internal(err.into()),
        }
    }
}
