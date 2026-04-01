//! Image processing pipeline for uploaded attachments.
//!
//! Resizes images to a single variant (max 2048px longest side) and
//! re-encodes them. GIF files are passed through without modification.

use std::time::Instant;

use image::ImageFormat;
use metrics::histogram;

use crate::error::WorkspaceError;

/// Result of processing an uploaded image.
#[derive(Debug)]
pub struct ProcessedImage {
    /// Processed image bytes.
    pub data: Vec<u8>,
    /// MIME content type of the output.
    pub content_type: String,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Maximum dimension (longest side) for processed images.
const MAX_DIMENSION: u32 = 2048;
/// JPEG output quality.
const JPEG_QUALITY: u8 = 85;

/// Processes an uploaded image: resizes if needed and re-encodes.
///
/// - JPEG/WebP inputs are re-encoded as JPEG (quality 85).
/// - PNG inputs are re-encoded as PNG (preserves alpha).
/// - GIF inputs are passed through without modification.
///
/// Returns an error if the image cannot be decoded.
pub fn process_image(data: &[u8], content_type: &str) -> Result<ProcessedImage, WorkspaceError> {
    let start = Instant::now();

    // GIF: pass through (animated GIF resize is complex and rarely needed)
    if content_type == "image/gif" {
        let img = image::load_from_memory(data)
            .map_err(|e| WorkspaceError::Internal(format!("failed to decode GIF: {e}")))?;
        histogram!("sober_attachment_image_processing_seconds")
            .record(start.elapsed().as_secs_f64());
        return Ok(ProcessedImage {
            data: data.to_vec(),
            content_type: "image/gif".into(),
            width: img.width(),
            height: img.height(),
        });
    }

    let img = image::load_from_memory(data)
        .map_err(|e| WorkspaceError::Internal(format!("failed to decode image: {e}")))?;

    // Determine if we need to resize
    let (orig_w, orig_h) = (img.width(), img.height());
    let img = if orig_w > MAX_DIMENSION || orig_h > MAX_DIMENSION {
        img.resize(
            MAX_DIMENSION,
            MAX_DIMENSION,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };

    let (width, height) = (img.width(), img.height());

    // Determine output format: PNG if input has alpha channel, otherwise JPEG
    let has_alpha = content_type == "image/png";

    let mut buf = Vec::new();

    let out_content_type = if has_alpha {
        let mut cursor = std::io::Cursor::new(&mut buf);
        img.write_to(&mut cursor, ImageFormat::Png)
            .map_err(|e| WorkspaceError::Internal(format!("failed to encode PNG: {e}")))?;
        "image/png".to_string()
    } else {
        let mut cursor = std::io::Cursor::new(&mut buf);
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, JPEG_QUALITY);
        img.write_with_encoder(encoder)
            .map_err(|e| WorkspaceError::Internal(format!("failed to encode JPEG: {e}")))?;
        "image/jpeg".to_string()
    };

    histogram!("sober_attachment_image_processing_seconds").record(start.elapsed().as_secs_f64());

    Ok(ProcessedImage {
        data: buf,
        content_type: out_content_type,
        width,
        height,
    })
}

/// Validates a content type against magic bytes in the file data.
///
/// Returns the detected MIME type if it matches an allowed type, or `None`
/// if the content type doesn't match any known format.
pub fn validate_content_type(data: &[u8]) -> Option<&'static str> {
    if data.len() < 4 {
        return None;
    }

    // JPEG: FF D8 FF
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }

    // PNG: 89 50 4E 47 0D 0A 1A 0A
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }

    // GIF: GIF87a or GIF89a
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return Some("image/gif");
    }

    // WebP: RIFF....WEBP
    if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        return Some("image/webp");
    }

    // PDF: %PDF
    if data.starts_with(b"%PDF") {
        return Some("application/pdf");
    }

    // MP3: ID3 or FF FB/FF F3/FF F2 (sync word)
    if data.starts_with(b"ID3") || (data[0] == 0xFF && (data[1] & 0xE0) == 0xE0) {
        return Some("audio/mpeg");
    }

    // WAV: RIFF....WAVE
    if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WAVE" {
        return Some("audio/wav");
    }

    // OGG: OggS
    if data.starts_with(b"OggS") {
        return Some("audio/ogg");
    }

    // MP4: ....ftyp (offset 4)
    if data.len() >= 8 && &data[4..8] == b"ftyp" {
        return Some("video/mp4");
    }

    // WebM: EBML header (1A 45 DF A3)
    if data.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        // Could be audio/webm or video/webm — default to video
        return Some("video/webm");
    }

    // Text-based formats: check if valid UTF-8
    if std::str::from_utf8(data).is_ok() {
        // Check for JSON
        let trimmed = data.iter().position(|&b| !b.is_ascii_whitespace());
        if let Some(pos) = trimmed {
            if data[pos] == b'{' || data[pos] == b'[' {
                return Some("application/json");
            }
            if data[pos] == b'<' {
                // Could be XML or HTML
                return Some("application/xml");
            }
        }
        // Default text
        return Some("text/plain");
    }

    None
}

/// Derives the attachment kind from a validated content type.
#[must_use]
pub fn derive_attachment_kind(content_type: &str) -> sober_core::types::AttachmentKind {
    use sober_core::types::AttachmentKind;
    if content_type.starts_with("image/") {
        AttachmentKind::Image
    } else if content_type.starts_with("audio/") {
        AttachmentKind::Audio
    } else if content_type.starts_with("video/") {
        AttachmentKind::Video
    } else {
        AttachmentKind::Document
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_jpeg_magic_bytes() {
        let data = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert_eq!(validate_content_type(&data), Some("image/jpeg"));
    }

    #[test]
    fn validate_png_magic_bytes() {
        let data = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(validate_content_type(&data), Some("image/png"));
    }

    #[test]
    fn validate_gif_magic_bytes() {
        let data = b"GIF89a\x00\x00";
        assert_eq!(validate_content_type(data), Some("image/gif"));
    }

    #[test]
    fn validate_pdf_magic_bytes() {
        let data = b"%PDF-1.4 some content";
        assert_eq!(validate_content_type(data), Some("application/pdf"));
    }

    #[test]
    fn validate_unknown_returns_none() {
        // Non-UTF-8 bytes that don't match any known magic signature
        let data = [0x80, 0x81, 0x82, 0x83];
        assert_eq!(validate_content_type(&data), None);
    }

    #[test]
    fn validate_too_short_returns_none() {
        let data = [0xFF, 0xD8];
        assert_eq!(validate_content_type(&data), None);
    }

    #[test]
    fn derive_kind_from_content_type() {
        use sober_core::types::AttachmentKind;
        assert_eq!(derive_attachment_kind("image/jpeg"), AttachmentKind::Image);
        assert_eq!(derive_attachment_kind("image/png"), AttachmentKind::Image);
        assert_eq!(derive_attachment_kind("audio/mpeg"), AttachmentKind::Audio);
        assert_eq!(derive_attachment_kind("video/mp4"), AttachmentKind::Video);
        assert_eq!(
            derive_attachment_kind("application/pdf"),
            AttachmentKind::Document
        );
        assert_eq!(
            derive_attachment_kind("text/plain"),
            AttachmentKind::Document
        );
    }

    #[test]
    fn process_small_jpeg() {
        // Create a minimal 2x2 JPEG in memory
        let img = image::DynamicImage::new_rgb8(2, 2);
        let mut buf = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buf);
        img.write_to(&mut cursor, ImageFormat::Jpeg).unwrap();

        let result = process_image(&buf, "image/jpeg").unwrap();
        assert_eq!(result.content_type, "image/jpeg");
        assert_eq!(result.width, 2);
        assert_eq!(result.height, 2);
    }

    #[test]
    fn process_png_preserves_format() {
        let img = image::DynamicImage::new_rgba8(4, 4);
        let mut buf = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buf);
        img.write_to(&mut cursor, ImageFormat::Png).unwrap();

        let result = process_image(&buf, "image/png").unwrap();
        assert_eq!(result.content_type, "image/png");
    }

    #[test]
    fn process_large_image_resizes() {
        let img = image::DynamicImage::new_rgb8(4096, 3072);
        let mut buf = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buf);
        img.write_to(&mut cursor, ImageFormat::Jpeg).unwrap();

        let result = process_image(&buf, "image/jpeg").unwrap();
        assert!(result.width <= MAX_DIMENSION);
        assert!(result.height <= MAX_DIMENSION);
    }
}
