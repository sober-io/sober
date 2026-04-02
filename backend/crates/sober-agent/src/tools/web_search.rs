//! Web search tool implementation.
//!
//! Uses a SearXNG instance to perform web searches and return formatted results.

use reqwest::Client;
use serde::Deserialize;
use sober_core::types::tool::{
    BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput, ToolVisibility,
};
use tracing::instrument;

use super::http_client;

/// Built-in tool that performs web searches via a SearXNG instance.
pub struct WebSearchTool {
    client: Client,
    base_url: String,
}

/// A single result from SearXNG.
#[derive(Debug, Deserialize)]
struct SearxResult {
    /// Title of the search result.
    title: String,
    /// URL of the search result.
    url: String,
    /// Snippet / content preview.
    #[serde(default)]
    content: String,
}

/// Top-level SearXNG JSON response.
#[derive(Debug, Deserialize)]
struct SearxResponse {
    /// List of search results.
    results: Vec<SearxResult>,
}

impl WebSearchTool {
    /// Creates a new web search tool targeting the given SearXNG instance.
    pub fn new(searxng_base_url: String) -> Self {
        Self {
            client: http_client(),
            base_url: searxng_base_url,
        }
    }

    /// Creates a web search tool with a pre-configured HTTP client (useful for testing).
    pub fn with_client(client: Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

/// Formats search results as a numbered list, respecting `max_results`.
fn format_results(results: &[SearxResult], max_results: usize) -> String {
    results
        .iter()
        .take(max_results)
        .enumerate()
        .map(|(i, r)| {
            format!(
                "{}. {}\n   URL: {}\n   {}",
                i + 1,
                r.title,
                r.url,
                r.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

impl Tool for WebSearchTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "web_search".to_owned(),
            description: "Search the web using SearXNG and return a list of results.".to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query."
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default 5)."
                    }
                },
                "required": ["query"]
            }),
            context_modifying: false,
            redacted: false,
            visibility: ToolVisibility::Public,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

impl WebSearchTool {
    /// Inner async implementation for [`Tool::execute`].
    #[instrument(skip(self, input), fields(tool.name = "web_search"))]
    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'query'".to_owned()))?;

        if query.is_empty() {
            return Err(ToolError::InvalidInput(
                "query must not be empty".to_owned(),
            ));
        }

        let max_results = input
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize;

        let url = format!("{}/search", self.base_url);
        let resp = self
            .client
            .get(&url)
            .query(&[("q", query), ("format", "json")])
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("HTTP request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(ToolError::ExecutionFailed(format!(
                "SearXNG returned status {}",
                resp.status()
            )));
        }

        let searx: SearxResponse = resp
            .json()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to parse response: {e}")))?;

        let output = if searx.results.is_empty() {
            "No results found.".to_owned()
        } else {
            format_results(&searx.results, max_results)
        };

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
        let tool = WebSearchTool::new("http://localhost:8080".to_owned());
        let meta = tool.metadata();
        assert_eq!(meta.name, "web_search");
        assert!(!meta.context_modifying);
        assert!(meta.description.contains("SearXNG"));

        // Schema has required query field
        let required = meta.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("query")));
    }

    #[tokio::test]
    async fn rejects_missing_query() {
        let tool = WebSearchTool::new("http://localhost:8080".to_owned());
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
        assert!(err.to_string().contains("query"));
    }

    #[tokio::test]
    async fn rejects_empty_query() {
        let tool = WebSearchTool::new("http://localhost:8080".to_owned());
        let result = tool.execute(serde_json::json!({"query": ""})).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn format_results_output() {
        let results = vec![
            SearxResult {
                title: "Result One".to_owned(),
                url: "https://example.com/1".to_owned(),
                content: "First result content.".to_owned(),
            },
            SearxResult {
                title: "Result Two".to_owned(),
                url: "https://example.com/2".to_owned(),
                content: "Second result content.".to_owned(),
            },
        ];

        let formatted = format_results(&results, 5);
        assert!(formatted.contains("1. Result One"));
        assert!(formatted.contains("URL: https://example.com/1"));
        assert!(formatted.contains("2. Result Two"));
    }

    #[test]
    fn format_results_respects_max() {
        let results = vec![
            SearxResult {
                title: "A".to_owned(),
                url: "https://a.com".to_owned(),
                content: String::new(),
            },
            SearxResult {
                title: "B".to_owned(),
                url: "https://b.com".to_owned(),
                content: String::new(),
            },
            SearxResult {
                title: "C".to_owned(),
                url: "https://c.com".to_owned(),
                content: String::new(),
            },
        ];

        let formatted = format_results(&results, 2);
        assert!(formatted.contains("1. A"));
        assert!(formatted.contains("2. B"));
        assert!(!formatted.contains("C"));
    }
}
