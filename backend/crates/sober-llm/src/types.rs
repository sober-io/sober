//! Request/response types for LLM operations.
//!
//! All types map directly to the
//! [OpenAI Chat Completions API](https://platform.openai.com/docs/api-reference/chat)
//! JSON format.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Completion request
// ---------------------------------------------------------------------------

/// A chat completion request (maps to `POST /chat/completions`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    /// Model identifier (e.g. `"anthropic/claude-sonnet-4"`).
    pub model: String,

    /// Conversation messages.
    pub messages: Vec<Message>,

    /// Tool definitions available to the model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,

    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Sampling temperature (0.0–2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Stop sequences.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,

    /// Whether to stream the response.
    #[serde(default)]
    pub stream: bool,
}

/// A single message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Role: `"system"`, `"user"`, `"assistant"`, or `"tool"`.
    pub role: String,

    /// Text content (absent when the assistant response is tool-calls only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Reasoning/thinking content returned by some providers (e.g. Kimi).
    /// Must be echoed back in subsequent messages when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,

    /// Tool calls made by the assistant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,

    /// For `role = "tool"` — the ID of the tool call this message responds to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// Create a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_owned(),
            content: Some(content.into()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_owned(),
            content: Some(content.into()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_owned(),
            content: Some(content.into()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create a tool result message.
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_owned(),
            content: Some(content.into()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// Tool definitions and calls
// ---------------------------------------------------------------------------

/// A tool definition (always `type: "function"` in the OpenAI format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool type — always `"function"`.
    pub r#type: String,

    /// Function metadata.
    pub function: FunctionDefinition,
}

/// Function metadata within a tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    /// Function name.
    pub name: String,

    /// Human-readable description.
    pub description: String,

    /// JSON Schema describing the function parameters.
    pub parameters: serde_json::Value,
}

/// A tool call emitted by the assistant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique ID for this tool call.
    pub id: String,

    /// Call type — always `"function"`.
    pub r#type: String,

    /// The function being called.
    pub function: FunctionCall,
}

/// A specific function invocation within a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    /// Function name.
    pub name: String,

    /// Arguments as a JSON string (the model may return partial/malformed JSON).
    pub arguments: String,
}

// ---------------------------------------------------------------------------
// Completion response
// ---------------------------------------------------------------------------

/// Response from a chat completion request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    /// Provider-assigned response ID.
    pub id: String,

    /// Response choices (usually exactly one).
    pub choices: Vec<Choice>,

    /// Token usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// A single completion choice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    /// Choice index (always 0 for our use).
    pub index: u32,

    /// The assistant's response message.
    pub message: Message,

    /// Why generation stopped: `"stop"`, `"tool_calls"`, `"length"`.
    pub finish_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Streaming types
// ---------------------------------------------------------------------------

/// A single chunk from a streaming completion (SSE `data:` payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    /// Provider-assigned response ID.
    pub id: String,

    /// Streaming choices.
    pub choices: Vec<StreamChoice>,

    /// Token usage (present in the final chunk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// A streaming choice within a chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChoice {
    /// Choice index.
    pub index: u32,

    /// Incremental message content.
    pub delta: MessageDelta,

    /// Present in the final chunk.
    pub finish_reason: Option<String>,
}

/// Incremental message fields in a streaming response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageDelta {
    /// Present in the first chunk only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,

    /// Incremental text content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Incremental tool call data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

/// Incremental tool call data in a streaming response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallDelta {
    /// Index of this tool call in the array (for multi-tool responses).
    pub index: u32,

    /// Tool call ID (present in the first delta for this tool call).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Incremental function data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<FunctionCallDelta>,
}

/// Incremental function call data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallDelta {
    /// Function name (present in the first delta).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Incremental argument string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

// ---------------------------------------------------------------------------
// Token usage
// ---------------------------------------------------------------------------

/// Token usage statistics.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Usage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u32,

    /// Tokens generated by the completion.
    pub completion_tokens: u32,

    /// Total tokens (prompt + completion).
    pub total_tokens: u32,
}

// ---------------------------------------------------------------------------
// Engine capabilities
// ---------------------------------------------------------------------------

/// Describes what an engine supports.
#[derive(Debug, Clone, Copy)]
pub struct EngineCapabilities {
    /// Whether the engine supports tool/function calling.
    pub supports_tools: bool,

    /// Whether the engine supports streaming responses.
    pub supports_streaming: bool,

    /// Whether the engine supports embedding generation.
    pub supports_embeddings: bool,

    /// Maximum context window size in tokens.
    pub max_context_tokens: u32,
}

// ---------------------------------------------------------------------------
// Embedding types
// ---------------------------------------------------------------------------

/// An embedding generation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    /// Model identifier for embedding generation.
    pub model: String,

    /// Input texts to embed.
    pub input: Vec<String>,
}

/// Response from an embedding generation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    /// Embedding results.
    pub data: Vec<EmbeddingData>,

    /// Token usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<EmbeddingUsage>,
}

/// A single embedding vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingData {
    /// Index of the input text.
    pub index: u32,

    /// The embedding vector.
    pub embedding: Vec<f32>,
}

/// Token usage for embedding requests.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EmbeddingUsage {
    /// Tokens consumed by the input.
    pub prompt_tokens: u32,

    /// Total tokens.
    pub total_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_request_serialization_roundtrip() {
        let req = CompletionRequest {
            model: "gpt-4o".to_owned(),
            messages: vec![
                Message::system("You are a helpful assistant."),
                Message::user("Hello!"),
            ],
            tools: vec![],
            max_tokens: Some(1024),
            temperature: Some(0.7),
            stop: vec![],
            stream: false,
        };

        let json = serde_json::to_string(&req).unwrap();
        let parsed: CompletionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "gpt-4o");
        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.max_tokens, Some(1024));
    }

    #[test]
    fn completion_response_deserialization() {
        let json = r#"{
            "id": "chatcmpl-123",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help you?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 8,
                "total_tokens": 18
            }
        }"#;

        let resp: CompletionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, "chatcmpl-123");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello! How can I help you?")
        );
        assert_eq!(resp.usage.unwrap().total_tokens, 18);
    }

    #[test]
    fn tool_call_response_deserialization() {
        let json = r#"{
            "id": "chatcmpl-456",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"London\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": null
        }"#;

        let resp: CompletionResponse = serde_json::from_str(json).unwrap();
        let tool_calls = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "get_weather");
        assert_eq!(tool_calls[0].function.arguments, r#"{"city":"London"}"#);
    }

    #[test]
    fn stream_chunk_deserialization() {
        let json = r#"{
            "id": "chatcmpl-789",
            "choices": [{
                "index": 0,
                "delta": {
                    "content": "Hello"
                },
                "finish_reason": null
            }]
        }"#;

        let chunk: StreamChunk = serde_json::from_str(json).unwrap();
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hello"));
        assert!(chunk.choices[0].finish_reason.is_none());
    }

    #[test]
    fn tool_definition_serialization() {
        let tool = ToolDefinition {
            r#type: "function".to_owned(),
            function: FunctionDefinition {
                name: "get_weather".to_owned(),
                description: "Get current weather".to_owned(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }),
            },
        };

        let json = serde_json::to_value(&tool).unwrap();
        assert_eq!(json["type"], "function");
        assert_eq!(json["function"]["name"], "get_weather");
    }

    #[test]
    fn optional_fields_omitted_when_none() {
        let req = CompletionRequest {
            model: "test".to_owned(),
            messages: vec![Message::user("hi")],
            tools: vec![],
            max_tokens: None,
            temperature: None,
            stop: vec![],
            stream: false,
        };

        let json = serde_json::to_value(&req).unwrap();
        assert!(!json.as_object().unwrap().contains_key("max_tokens"));
        assert!(!json.as_object().unwrap().contains_key("temperature"));
        assert!(!json.as_object().unwrap().contains_key("tools"));
        assert!(!json.as_object().unwrap().contains_key("stop"));
    }

    #[test]
    fn embedding_response_deserialization() {
        let json = r#"{
            "data": [{
                "index": 0,
                "embedding": [0.1, 0.2, 0.3]
            }],
            "usage": {
                "prompt_tokens": 5,
                "total_tokens": 5
            }
        }"#;

        let resp: EmbeddingResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.len(), 1);
        assert_eq!(resp.data[0].embedding, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn message_constructors() {
        let sys = Message::system("be helpful");
        assert_eq!(sys.role, "system");
        assert_eq!(sys.content.as_deref(), Some("be helpful"));

        let usr = Message::user("hello");
        assert_eq!(usr.role, "user");

        let asst = Message::assistant("hi there");
        assert_eq!(asst.role, "assistant");

        let tool = Message::tool("call_123", r#"{"result": 42}"#);
        assert_eq!(tool.role, "tool");
        assert_eq!(tool.tool_call_id.as_deref(), Some("call_123"));
    }
}
