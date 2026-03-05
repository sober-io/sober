# 007 — sober-llm

**Date:** 2026-03-06

Multi-provider LLM abstraction layer for Sober. Uses the **OpenAI-compatible API**
as the wire format, enabling any compatible provider (OpenRouter, Ollama, OpenAI,
Anthropic via proxy, vLLM, Together, etc.) to be plugged in via configuration.
Depends on `sober-core` only.

---

## Design Rationale

The OpenAI Chat Completions API has become the de facto standard. Nearly every LLM
provider offers an OpenAI-compatible endpoint. By building against this format:

- **OpenRouter on day one** — access Claude, GPT-4, Llama, Mistral, Gemini, and
  dozens more models through a single API key and base URL.
- **Ollama for local dev** — runs locally with an OpenAI-compatible endpoint,
  no API key needed.
- **No provider-specific code** — one client implementation handles all providers.
- **Future-proof** — new providers just need a base URL and API key.

---

## LLM Engine Trait

```rust
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
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, LlmError>;

    /// Engine capabilities (supports tools? streaming? embeddings?)
    fn capabilities(&self) -> EngineCapabilities;

    /// Model identifier string.
    fn model_id(&self) -> &str;
}
```

---

## Request/Response Types

These map directly to the OpenAI Chat Completions API format.

### CompletionRequest

| Field            | Type                  | OpenAI field       |
|------------------|-----------------------|--------------------|
| `model`          | `String`              | `model`            |
| `messages`       | `Vec<Message>`        | `messages`         |
| `tools`          | `Vec<ToolDefinition>` | `tools`            |
| `max_tokens`     | `Option<u32>`         | `max_tokens`       |
| `temperature`    | `Option<f32>`         | `temperature`      |
| `stop`           | `Vec<String>`         | `stop`             |
| `stream`         | `bool`                | `stream`           |

### Message

```rust
pub struct Message {
    pub role: String,        // "system", "user", "assistant", "tool"
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,  // for role="tool" responses
}
```

### ToolDefinition

```rust
pub struct ToolDefinition {
    pub r#type: String,  // always "function"
    pub function: FunctionDefinition,
}

pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,  // JSON Schema
}
```

### ToolCall

```rust
pub struct ToolCall {
    pub id: String,
    pub r#type: String,  // "function"
    pub function: FunctionCall,
}

pub struct FunctionCall {
    pub name: String,
    pub arguments: String,  // JSON string
}
```

### CompletionResponse

| Field     | Type                | OpenAI field          |
|-----------|---------------------|-----------------------|
| `id`      | `String`            | `id`                  |
| `choices` | `Vec<Choice>`       | `choices`             |
| `usage`   | `Option<Usage>`     | `usage`               |

### Choice

| Field          | Type                | Notes                        |
|----------------|---------------------|------------------------------|
| `index`        | `u32`               | Always 0 for our use         |
| `message`      | `Message`           | The assistant's response     |
| `finish_reason`| `Option<String>`    | "stop", "tool_calls", "length" |

### StreamChunk (SSE `data:` payload)

```rust
pub struct StreamChunk {
    pub id: String,
    pub choices: Vec<StreamChoice>,
    pub usage: Option<Usage>,  // present in final chunk
}

pub struct StreamChoice {
    pub index: u32,
    pub delta: MessageDelta,
    pub finish_reason: Option<String>,
}

pub struct MessageDelta {
    pub role: Option<String>,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

pub struct ToolCallDelta {
    pub index: u32,
    pub id: Option<String>,
    pub function: Option<FunctionCallDelta>,
}

pub struct FunctionCallDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}
```

### Usage

| Field             | Type  |
|-------------------|-------|
| `prompt_tokens`   | `u32` |
| `completion_tokens` | `u32` |
| `total_tokens`    | `u32` |

### EngineCapabilities

| Field                | Type   |
|----------------------|--------|
| `supports_tools`     | `bool` |
| `supports_streaming` | `bool` |
| `supports_embeddings`| `bool` |
| `max_context_tokens` | `u32`  |

---

## OpenAI-Compatible Client

A single `OpenAiCompatibleEngine` struct handles all providers:

```rust
pub struct OpenAiCompatibleEngine {
    client: reqwest::Client,
    base_url: String,        // e.g., "https://openrouter.ai/api/v1"
    api_key: Option<String>, // None for local Ollama
    model: String,           // e.g., "anthropic/claude-sonnet-4"
    default_max_tokens: u32,
}
```

### Provider Configuration Examples

| Provider   | Base URL                          | Model                          |
|------------|-----------------------------------|--------------------------------|
| OpenRouter | `https://openrouter.ai/api/v1`   | `anthropic/claude-sonnet-4`    |
| OpenAI     | `https://api.openai.com/v1`      | `gpt-4o`                       |
| Ollama     | `http://localhost:11434/v1`       | `llama3.1`                     |
| Together   | `https://api.together.xyz/v1`    | `meta-llama/Llama-3-70b`      |
| vLLM       | `http://localhost:8000/v1`       | `mistralai/Mistral-7B`        |

All configured via environment variables:

```env
LLM_BASE_URL=https://openrouter.ai/api/v1
LLM_API_KEY=sk-or-...
LLM_MODEL=anthropic/claude-sonnet-4
LLM_MAX_TOKENS=4096
```

### Endpoints Used

| Operation   | Endpoint                     | Notes                              |
|-------------|------------------------------|------------------------------------|
| Completions | `POST {base_url}/chat/completions` | With `stream: false`          |
| Streaming   | `POST {base_url}/chat/completions` | With `stream: true`, SSE response |
| Embeddings  | `POST {base_url}/embeddings` | Not all providers support this     |

### Streaming (SSE)

OpenAI streaming format:
- Each line: `data: {json}\n\n`
- Final line: `data: [DONE]\n\n`
- Parse each JSON payload as `StreamChunk`
- Accumulate `MessageDelta` chunks to build the full response

### Rate Limiting

- Detect 429 status
- Parse `retry-after` header (seconds) or `x-ratelimit-reset-*` headers
- Exponential backoff with jitter (configurable: max 3 retries, base 1s)
- Surface as `LlmError::RateLimited { retry_after }`

### OpenRouter-Specific Headers

When `base_url` contains `openrouter.ai`, add:
- `HTTP-Referer: <configurable site URL>` (for OpenRouter rankings)
- `X-Title: Sober` (for OpenRouter dashboard)

These are optional and only improve the OpenRouter developer experience.

---

## Embeddings

Embeddings support depends on the provider:
- **OpenAI / OpenRouter:** `POST /embeddings` with `model` and `input`
- **Ollama:** `POST /embeddings` (same format)
- **Providers without embeddings:** `embed()` returns `LlmError::Unsupported`

For v1, configure a separate `EMBEDDING_MODEL` (e.g., `openai/text-embedding-3-small`
via OpenRouter, or a local Ollama model). The engine detects whether the configured
provider supports embeddings and reports it via `capabilities()`.

---

## Error Type

```rust
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("API error (HTTP {status}): {message}")]
    ApiError { status: u16, message: String },

    #[error("Rate limited")]
    RateLimited { retry_after: Option<Duration> },

    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("Stream error: {0}")]
    StreamError(String),

    #[error("Unsupported: {0}")]
    Unsupported(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}
```

Maps to `AppError::Internal` at the API boundary.

---

## Dependencies

| Crate         | Purpose                                     |
|---------------|---------------------------------------------|
| `sober-core`  | Shared types, error handling                |
| `reqwest`     | HTTP client (rustls-tls, json, stream)      |
| `tokio`       | Async runtime                               |
| `serde`       | Serialization                               |
| `serde_json`  | JSON handling                               |
| `futures`     | `Stream` trait                              |
| `async-trait` | Async trait support                         |
| `tracing`     | Structured logging                          |
| `thiserror`   | Error types                                 |
