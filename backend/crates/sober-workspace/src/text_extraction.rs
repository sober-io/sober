//! Text extraction from uploaded documents.
//!
//! Extracts searchable text content from supported file types (currently PDF).
//! Returns `None` for unsupported types.

use crate::error::WorkspaceError;

/// Extracts text content from a document.
///
/// Currently supports:
/// - `application/pdf` — extracts text via pdf-extract
/// - `text/plain`, `text/csv`, `text/markdown`, `application/json`, `application/xml` — UTF-8 decode
///
/// Returns `Ok(None)` for unsupported content types.
/// Returns `Ok(Some(text))` with the extracted text.
/// Returns `Err` if extraction fails for a supported type.
pub fn extract_text(data: &[u8], content_type: &str) -> Result<Option<String>, WorkspaceError> {
    match content_type {
        "application/pdf" => extract_pdf_text(data).map(Some),
        "text/plain" | "text/csv" | "text/markdown" | "application/json" | "application/xml" => {
            // Text-based formats: decode as UTF-8
            let text = std::str::from_utf8(data)
                .map_err(|e| WorkspaceError::Internal(format!("invalid UTF-8: {e}")))?;
            Ok(Some(text.to_string()))
        }
        _ => Ok(None),
    }
}

/// Extracts text from a PDF document.
fn extract_pdf_text(data: &[u8]) -> Result<String, WorkspaceError> {
    pdf_extract::extract_text_from_mem(data)
        .map_err(|e| WorkspaceError::Internal(format!("PDF text extraction failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_type_returns_none() {
        let result = extract_text(b"some data", "image/jpeg").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn plain_text_extraction() {
        let text = "Hello, world!";
        let result = extract_text(text.as_bytes(), "text/plain").unwrap();
        assert_eq!(result, Some("Hello, world!".to_string()));
    }

    #[test]
    fn json_text_extraction() {
        let json = r#"{"key": "value"}"#;
        let result = extract_text(json.as_bytes(), "application/json").unwrap();
        assert_eq!(result, Some(json.to_string()));
    }

    #[test]
    fn invalid_utf8_returns_error() {
        let invalid = [0xFF, 0xFE, 0x00, 0x01];
        let result = extract_text(&invalid, "text/plain");
        assert!(result.is_err());
    }
}
