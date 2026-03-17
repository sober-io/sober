//! URL fetching tool implementation.
//!
//! Fetches the content of a URL, validates content type and size, strips HTML
//! tags when applicable, and truncates output to fit within LLM context limits.

use std::time::Duration;

use reqwest::Client;
use sober_core::types::tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};

/// Maximum response body size in bytes (10 MB).
const MAX_BODY_SIZE: usize = 10_485_760;

/// Maximum output length in characters sent back to the LLM.
const MAX_OUTPUT_LEN: usize = 8000;

/// HTTP request timeout.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Content-type prefixes that this tool is willing to fetch.
///
/// Any `text/*` content is allowed, plus common structured formats that LLMs
/// can process as text.
const ALLOWED_CONTENT_TYPES: &[&str] = &[
    "text/",
    "application/json",
    "application/xml",
    "application/xhtml+xml",
    "application/javascript",
    "application/x-yaml",
    "application/yaml",
    "application/toml",
    "application/csv",
    "application/ld+json",
    "application/rss+xml",
    "application/atom+xml",
];

/// Built-in tool that fetches the content of a URL.
pub struct FetchUrlTool {
    client: Client,
}

impl FetchUrlTool {
    /// Creates a new fetch-url tool with a default HTTP client.
    pub fn new() -> Self {
        // reqwest client construction only fails if the TLS backend fails to
        // initialize, which is unrecoverable. Panicking is appropriate here.
        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("failed to build reqwest client");
        Self { client }
    }

    /// Creates a fetch-url tool with a pre-configured HTTP client (useful for testing).
    pub fn with_client(client: Client) -> Self {
        Self { client }
    }
}

impl Default for FetchUrlTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Checks whether a `Content-Type` header value is in the allowed list.
///
/// Performs a case-insensitive prefix match so that charset parameters
/// (e.g. `text/html; charset=utf-8`) are accepted.
fn is_content_type_allowed(ct: &str) -> bool {
    let lower = ct.to_ascii_lowercase();
    ALLOWED_CONTENT_TYPES
        .iter()
        .any(|allowed| lower.starts_with(allowed))
}

/// Strips HTML tags and non-visible content from raw HTML.
///
/// Removes `<script>` and `<style>` blocks entirely, converts block-level
/// elements to newlines, and collapses excessive whitespace.
fn strip_html_tags(html: &str) -> String {
    let mut output = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut tag_name = String::new();
    let mut collecting_tag_name = false;

    // Block-level elements that produce newlines.
    let block_elements = ["p", "div", "h1", "h2", "h3", "br", "li", "tr"];

    for ch in html.chars() {
        if ch == '<' {
            in_tag = true;
            tag_name.clear();
            collecting_tag_name = true;
            continue;
        }

        if in_tag {
            if ch == '>' {
                in_tag = false;
                collecting_tag_name = false;
                let lower_tag = tag_name.to_ascii_lowercase();

                // Handle script/style open/close.
                if lower_tag == "script" {
                    in_script = true;
                } else if lower_tag == "/script" {
                    in_script = false;
                } else if lower_tag == "style" {
                    in_style = true;
                } else if lower_tag == "/style" {
                    in_style = false;
                }

                // Emit newline for block elements (both open and close).
                let bare = lower_tag.trim_start_matches('/');
                if block_elements.contains(&bare) {
                    output.push('\n');
                }
            } else if collecting_tag_name {
                // Allow '/' as the first character (closing tags like </script>).
                if ch == '/' && tag_name.is_empty() {
                    tag_name.push(ch);
                } else if ch.is_whitespace() || ch == '/' {
                    collecting_tag_name = false;
                } else {
                    tag_name.push(ch);
                }
            }
            continue;
        }

        // Skip content inside script/style blocks.
        if in_script || in_style {
            continue;
        }

        output.push(ch);
    }

    // Collapse runs of whitespace into single space/newline.
    collapse_whitespace(&output)
}

/// Collapses runs of whitespace: multiple blank lines become one, runs of
/// spaces/tabs become a single space.
fn collapse_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_newline = false;
    let mut prev_space = false;

    for ch in s.chars() {
        if ch == '\n' {
            if !prev_newline {
                result.push('\n');
            }
            prev_newline = true;
            prev_space = false;
        } else if ch.is_whitespace() {
            if !prev_space && !prev_newline {
                result.push(' ');
            }
            prev_space = true;
        } else {
            prev_newline = false;
            prev_space = false;
            result.push(ch);
        }
    }

    result.trim().to_owned()
}

impl Tool for FetchUrlTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "fetch_url".to_owned(),
            description:
                "Fetch the content of a URL. Supports text-based content types including HTML, plain text, JSON, XML, YAML, and more."
                    .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch (must start with http:// or https://)."
                    }
                },
                "required": ["url"]
            }),
            context_modifying: false,
            internal: false,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

impl FetchUrlTool {
    /// Inner async implementation for [`Tool::execute`].
    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'url'".to_owned()))?;

        if url.is_empty() {
            return Err(ToolError::InvalidInput("url must not be empty".to_owned()));
        }

        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ToolError::InvalidInput(
                "url must start with http:// or https://".to_owned(),
            ));
        }

        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("HTTP request failed: {e}")))?;

        // Check content type.
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream");

        if !is_content_type_allowed(content_type) {
            return Err(ToolError::ExecutionFailed(format!(
                "unsupported content type: {content_type}"
            )));
        }

        let is_html = content_type.to_ascii_lowercase().starts_with("text/html");

        // Check content length if the server provides it.
        if let Some(cl) = resp.content_length()
            && cl as usize > MAX_BODY_SIZE
        {
            return Err(ToolError::ExecutionFailed(format!(
                "response too large: {cl} bytes (max {MAX_BODY_SIZE})"
            )));
        }

        let body = resp
            .bytes()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to read body: {e}")))?;

        if body.len() > MAX_BODY_SIZE {
            return Err(ToolError::ExecutionFailed(format!(
                "response too large: {} bytes (max {MAX_BODY_SIZE})",
                body.len()
            )));
        }

        let text = String::from_utf8_lossy(&body);

        let mut output = if is_html {
            strip_html_tags(&text)
        } else {
            text.into_owned()
        };

        // Truncate to MAX_OUTPUT_LEN.
        if output.len() > MAX_OUTPUT_LEN {
            output.truncate(MAX_OUTPUT_LEN);
            output.push_str("\n\n[truncated]");
        }

        Ok(ToolOutput {
            content: output,
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_correctness() {
        let tool = FetchUrlTool::new();
        let meta = tool.metadata();
        assert_eq!(meta.name, "fetch_url");
        assert!(!meta.context_modifying);

        let required = meta.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("url")));
    }

    #[tokio::test]
    async fn rejects_missing_url() {
        let tool = FetchUrlTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
        assert!(err.to_string().contains("url"));
    }

    #[tokio::test]
    async fn rejects_empty_url() {
        let tool = FetchUrlTool::new();
        let result = tool.execute(serde_json::json!({"url": ""})).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
        assert!(err.to_string().contains("empty"));
    }

    #[tokio::test]
    async fn rejects_non_http_url() {
        let tool = FetchUrlTool::new();
        let result = tool
            .execute(serde_json::json!({"url": "ftp://example.com/file"}))
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
        assert!(err.to_string().contains("http"));
    }

    #[test]
    fn strip_html_basic() {
        let html = "<p>Hello <b>world</b></p>";
        let text = strip_html_tags(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
        assert!(!text.contains('<'));
        assert!(!text.contains('>'));
    }

    #[test]
    fn strip_html_removes_scripts() {
        let html = "<p>Before</p><script>alert('xss');</script><p>After</p>";
        let text = strip_html_tags(html);
        assert!(text.contains("Before"));
        assert!(text.contains("After"));
        assert!(!text.contains("alert"));
        assert!(!text.contains("script"));
    }

    #[test]
    fn strip_html_removes_styles() {
        let html = "<p>Visible</p><style>body { color: red; }</style><p>Also visible</p>";
        let text = strip_html_tags(html);
        assert!(text.contains("Visible"));
        assert!(text.contains("Also visible"));
        assert!(!text.contains("color"));
        assert!(!text.contains("style"));
    }

    #[test]
    fn content_type_filtering() {
        // All text/* types are allowed.
        assert!(is_content_type_allowed("text/html"));
        assert!(is_content_type_allowed("text/html; charset=utf-8"));
        assert!(is_content_type_allowed("text/plain"));
        assert!(is_content_type_allowed("text/csv"));
        assert!(is_content_type_allowed("text/xml"));
        assert!(is_content_type_allowed("text/markdown"));
        assert!(is_content_type_allowed("TEXT/HTML; charset=utf-8"));

        // Specific application types are allowed.
        assert!(is_content_type_allowed("application/json"));
        assert!(is_content_type_allowed("Application/JSON"));
        assert!(is_content_type_allowed("application/xml"));
        assert!(is_content_type_allowed("application/yaml"));
        assert!(is_content_type_allowed("application/javascript"));
        assert!(is_content_type_allowed("application/rss+xml"));
        assert!(is_content_type_allowed(
            "application/ld+json; charset=utf-8"
        ));

        // Binary types are rejected.
        assert!(!is_content_type_allowed("application/octet-stream"));
        assert!(!is_content_type_allowed("image/png"));
        assert!(!is_content_type_allowed("application/pdf"));
        assert!(!is_content_type_allowed("video/mp4"));
    }
}
