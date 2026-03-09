//! LLM engine trait — the core abstraction for all providers.

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use crate::error::LlmError;
use crate::types::{CompletionRequest, CompletionResponse, EngineCapabilities, StreamChunk};

/// Core abstraction for LLM providers.
///
/// Implementations include [`OpenAiCompatibleEngine`](crate::client::OpenAiCompatibleEngine)
/// for HTTP providers and [`AcpEngine`](crate::acp::AcpEngine) for local ACP agents.
///
/// This trait is object-safe — it can be used as `dyn LlmEngine`.
#[async_trait]
pub trait LlmEngine: Send + Sync {
    /// Send a completion request and get a full response.
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError>;

    /// Send a completion request and stream response chunks.
    async fn stream(
        &self,
        req: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send>>, LlmError>;

    /// Generate embeddings for a batch of texts.
    ///
    /// Returns [`LlmError::Unsupported`] if the engine does not support embeddings.
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, LlmError>;

    /// Engine capabilities (tool support, streaming, embeddings, context window).
    fn capabilities(&self) -> EngineCapabilities;

    /// Model identifier string (e.g. `"anthropic/claude-sonnet-4"` or `"acp:claude-code/1.0"`).
    fn model_id(&self) -> &str;
}
