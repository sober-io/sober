//! OpenAI-compatible HTTP client engine.
//!
//! A single [`OpenAiCompatibleEngine`] implementation handles all providers
//! that expose an OpenAI-compatible API: OpenRouter, OpenAI, Ollama, Together,
//! vLLM, etc.

use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use futures::Stream;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use reqwest::{Client, StatusCode};
use sober_core::config::LlmConfig;
use tracing::debug;

use crate::engine::LlmEngine;
use crate::error::LlmError;
use crate::streaming::parse_sse_stream;
use crate::types::{
    CompletionRequest, CompletionResponse, EmbeddingRequest, EmbeddingResponse, EngineCapabilities,
    StreamChunk,
};

/// An LLM engine backed by an OpenAI-compatible HTTP API.
///
/// Works with any provider that implements the OpenAI Chat Completions format:
/// OpenRouter, OpenAI, Ollama, Together, vLLM, etc.
pub struct OpenAiCompatibleEngine {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
    embedding_model: String,
    default_max_tokens: u32,
    is_openrouter: bool,
}

impl std::fmt::Debug for OpenAiCompatibleEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiCompatibleEngine")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("embedding_model", &self.embedding_model)
            .field("default_max_tokens", &self.default_max_tokens)
            .field("is_openrouter", &self.is_openrouter)
            .finish()
    }
}

impl OpenAiCompatibleEngine {
    /// Create a new engine from explicit parameters.
    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
        embedding_model: impl Into<String>,
        max_tokens: u32,
    ) -> Self {
        let base_url = base_url.into();
        let is_openrouter = base_url.contains("openrouter.ai");

        Self {
            client: Client::new(),
            is_openrouter,
            base_url,
            api_key,
            model: model.into(),
            embedding_model: embedding_model.into(),
            default_max_tokens: max_tokens,
        }
    }

    /// Create a new engine from sober-core's [`LlmConfig`].
    pub fn from_config(config: &LlmConfig) -> Self {
        Self::new(
            &config.base_url,
            config.api_key.clone(),
            &config.model,
            &config.embedding_model,
            config.max_tokens,
        )
    }

    /// Build common headers for all requests.
    fn build_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();

        if let Some(ref key) = self.api_key {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {key}"))
                    .expect("API key contains invalid header characters"),
            );
        }

        headers.insert("Content-Type", HeaderValue::from_static("application/json"));

        // OpenRouter-specific headers for ranking/dashboard.
        if self.is_openrouter {
            headers.insert(
                "HTTP-Referer",
                HeaderValue::from_static("https://github.com/sober-io/sober"),
            );
            headers.insert("X-Title", HeaderValue::from_static("Sober"));
        }

        headers
    }

    /// Parse rate-limit response headers into a retry duration.
    fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
        // Try standard `retry-after` header (seconds).
        if let Some(val) = headers.get("retry-after")
            && let Ok(s) = val.to_str()
            && let Ok(secs) = s.parse::<u64>()
        {
            return Some(Duration::from_secs(secs));
        }
        None
    }

    /// Handle non-success HTTP responses.
    async fn handle_error_response(response: reqwest::Response) -> LlmError {
        let status = response.status();
        let headers = response.headers().clone();

        if status == StatusCode::TOO_MANY_REQUESTS {
            return LlmError::RateLimited {
                retry_after: Self::parse_retry_after(&headers),
            };
        }

        let body = response.text().await.unwrap_or_default();

        // Try to extract error message from JSON body.
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(String::from)
            })
            .unwrap_or(body);

        LlmError::ApiError {
            status: status.as_u16(),
            message,
        }
    }
}

#[async_trait]
impl LlmEngine for OpenAiCompatibleEngine {
    async fn complete(&self, mut req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        req.stream = false;
        if req.max_tokens.is_none() {
            req.max_tokens = Some(self.default_max_tokens);
        }

        let url = format!("{}/chat/completions", self.base_url);
        debug!(url = %url, model = %req.model, "sending completion request");

        let response = self
            .client
            .post(&url)
            .headers(self.build_headers())
            .json(&req)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Self::handle_error_response(response).await);
        }

        let body = response.text().await?;
        serde_json::from_str(&body).map_err(|e| LlmError::InvalidResponse(e.to_string()))
    }

    async fn stream(
        &self,
        mut req: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send>>, LlmError> {
        req.stream = true;
        if req.max_tokens.is_none() {
            req.max_tokens = Some(self.default_max_tokens);
        }

        let url = format!("{}/chat/completions", self.base_url);
        debug!(url = %url, model = %req.model, "sending streaming request");

        let response = self
            .client
            .post(&url)
            .headers(self.build_headers())
            .json(&req)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Self::handle_error_response(response).await);
        }

        Ok(Box::pin(parse_sse_stream(response)))
    }

    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, LlmError> {
        let url = format!("{}/embeddings", self.base_url);
        debug!(url = %url, model = %self.embedding_model, count = texts.len(), "sending embedding request");

        let req = EmbeddingRequest {
            model: self.embedding_model.clone(),
            input: texts.iter().map(|s| (*s).to_owned()).collect(),
        };

        let response = self
            .client
            .post(&url)
            .headers(self.build_headers())
            .json(&req)
            .send()
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Err(LlmError::Unsupported(
                "embedding endpoint not available for this provider".to_owned(),
            ));
        }

        if !response.status().is_success() {
            return Err(Self::handle_error_response(response).await);
        }

        let body = response.text().await?;
        let resp: EmbeddingResponse =
            serde_json::from_str(&body).map_err(|e| LlmError::InvalidResponse(e.to_string()))?;

        // Sort by index and extract vectors.
        let mut data = resp.data;
        data.sort_by_key(|d| d.index);
        Ok(data.into_iter().map(|d| d.embedding).collect())
    }

    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities {
            supports_tools: true,
            supports_streaming: true,
            supports_embeddings: true,
            max_context_tokens: 128_000,
        }
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openrouter_detection() {
        let engine = OpenAiCompatibleEngine::new(
            "https://openrouter.ai/api/v1",
            Some("sk-test".to_owned()),
            "test-model",
            "test-embed",
            4096,
        );
        assert!(engine.is_openrouter);

        let engine2 = OpenAiCompatibleEngine::new(
            "https://api.openai.com/v1",
            Some("sk-test".to_owned()),
            "gpt-4o",
            "text-embedding-3-small",
            4096,
        );
        assert!(!engine2.is_openrouter);
    }

    #[test]
    fn openrouter_headers_present() {
        let engine = OpenAiCompatibleEngine::new(
            "https://openrouter.ai/api/v1",
            Some("sk-test".to_owned()),
            "test-model",
            "test-embed",
            4096,
        );
        let headers = engine.build_headers();
        assert!(headers.contains_key("HTTP-Referer"));
        assert!(headers.contains_key("X-Title"));
        assert!(headers.contains_key(AUTHORIZATION));
    }

    #[test]
    fn non_openrouter_headers_absent() {
        let engine = OpenAiCompatibleEngine::new(
            "http://localhost:11434/v1",
            None,
            "llama3.1",
            "nomic-embed-text",
            4096,
        );
        let headers = engine.build_headers();
        assert!(!headers.contains_key("HTTP-Referer"));
        assert!(!headers.contains_key("X-Title"));
        assert!(!headers.contains_key(AUTHORIZATION));
    }

    #[test]
    fn from_config() {
        let config = LlmConfig {
            base_url: "https://openrouter.ai/api/v1".to_owned(),
            api_key: Some("sk-test".to_owned()),
            model: "anthropic/claude-sonnet-4".to_owned(),
            max_tokens: 8192,
            embedding_model: "text-embedding-3-small".to_owned(),
        };
        let engine = OpenAiCompatibleEngine::from_config(&config);
        assert_eq!(engine.model_id(), "anthropic/claude-sonnet-4");
        assert_eq!(engine.default_max_tokens, 8192);
        assert!(engine.is_openrouter);
    }

    #[test]
    fn parse_retry_after_header() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("30"));
        let duration = OpenAiCompatibleEngine::parse_retry_after(&headers);
        assert_eq!(duration, Some(Duration::from_secs(30)));
    }

    #[test]
    fn parse_retry_after_missing() {
        let headers = HeaderMap::new();
        let duration = OpenAiCompatibleEngine::parse_retry_after(&headers);
        assert!(duration.is_none());
    }
}
