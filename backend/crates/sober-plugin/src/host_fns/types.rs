//! Request and response types for the WASM host function boundary.
//!
//! All host functions communicate with plugins via JSON-serialized structs.
//! Request types are deserialized from plugin input; response types are
//! serialized back to plugin output.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Log
// ---------------------------------------------------------------------------

/// Input for `host_log`.
#[derive(Debug, Deserialize)]
pub(crate) struct LogRequest {
    pub level: String,
    pub message: String,
    #[serde(default)]
    pub fields: HashMap<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Key-value storage
// ---------------------------------------------------------------------------

/// Input for `host_kv_get`.
#[derive(Debug, Deserialize)]
pub(crate) struct KvGetRequest {
    pub key: String,
}

/// Output for `host_kv_get`.
#[derive(Debug, Serialize)]
pub(crate) struct KvGetResponse {
    pub value: Option<serde_json::Value>,
}

/// Input for `host_kv_set`.
#[derive(Debug, Deserialize)]
pub(crate) struct KvSetRequest {
    pub key: String,
    pub value: serde_json::Value,
}

/// Input for `host_kv_delete`.
#[derive(Debug, Deserialize)]
pub(crate) struct KvDeleteRequest {
    pub key: String,
}

/// Input for `host_kv_list`.
#[derive(Debug, Deserialize)]
pub(crate) struct KvListRequest {
    #[serde(default)]
    pub prefix: Option<String>,
}

/// Output for `host_kv_list`.
#[derive(Debug, Serialize)]
pub(crate) struct KvListResponse {
    pub keys: Vec<String>,
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

/// Input for `host_http_request`.
#[derive(Debug, Deserialize)]
pub(crate) struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
}

/// Output for `host_http_request`.
#[derive(Debug, Serialize)]
pub(crate) struct HttpResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub body: String,
}

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------

/// Input for `host_read_secret`.
#[derive(Debug, Deserialize)]
pub(crate) struct ReadSecretRequest {
    pub name: String,
}

/// Output for `host_read_secret`.
#[derive(Debug, Serialize)]
pub(crate) struct ReadSecretResponse {
    pub value: String,
}

// ---------------------------------------------------------------------------
// Tool calls
// ---------------------------------------------------------------------------

/// Input for `host_call_tool`.
#[derive(Debug, Deserialize)]
pub(crate) struct CallToolRequest {
    pub tool: String,
    #[serde(default)]
    pub input: serde_json::Value,
}

/// Output for `host_call_tool`.
#[derive(Debug, Serialize)]
pub(crate) struct CallToolResponse {
    pub output: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

/// Input for `host_emit_metric`.
#[derive(Debug, Deserialize)]
pub(crate) struct EmitMetricRequest {
    pub name: String,
    pub kind: String,
    pub value: f64,
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

/// Input for `host_memory_query`.
#[derive(Debug, Deserialize)]
pub(crate) struct MemoryQueryRequest {
    pub query: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Input for `host_memory_write`.
#[derive(Debug, Deserialize)]
pub(crate) struct MemoryWriteRequest {
    pub content: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Output for `host_memory_query`.
#[derive(Debug, Serialize)]
pub(crate) struct MemoryQueryResponse {
    pub results: Vec<crate::backends::MemoryHit>,
}

// ---------------------------------------------------------------------------
// Conversation
// ---------------------------------------------------------------------------

/// Input for `host_conversation_read`.
#[derive(Debug, Deserialize)]
pub(crate) struct ConversationReadRequest {
    pub conversation_id: String,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Output for `host_conversation_read`.
#[derive(Debug, Serialize)]
pub(crate) struct ConversationReadResponse {
    pub messages: Vec<crate::backends::ConversationMessage>,
}

// ---------------------------------------------------------------------------
// Scheduling
// ---------------------------------------------------------------------------

/// Input for `host_schedule`.
#[derive(Debug, Deserialize)]
pub(crate) struct ScheduleRequest {
    /// Cron expression or interval (e.g. "*/5 * * * *" or "30s").
    pub schedule: String,
    /// Payload to deliver when the job fires.
    pub payload: serde_json::Value,
}

/// Output for `host_schedule`.
#[derive(Debug, Serialize)]
pub(crate) struct ScheduleResponse {
    pub job_id: String,
}

// ---------------------------------------------------------------------------
// Filesystem
// ---------------------------------------------------------------------------

/// Input for `host_fs_read`.
#[derive(Debug, Deserialize)]
pub(crate) struct FsReadRequest {
    pub path: String,
}

/// Input for `host_fs_write`.
#[derive(Debug, Deserialize)]
pub(crate) struct FsWriteRequest {
    pub path: String,
    pub content: String,
}

/// Output for `host_fs_read`.
#[derive(Debug, Serialize)]
pub(crate) struct FsReadResponse {
    pub content: String,
}

// ---------------------------------------------------------------------------
// LLM
// ---------------------------------------------------------------------------

/// Input for `host_llm_complete`.
#[derive(Debug, Deserialize)]
pub(crate) struct LlmCompleteRequest {
    pub prompt: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

/// Output for `host_llm_complete`.
#[derive(Debug, Serialize)]
pub(crate) struct LlmCompleteResponse {
    pub text: String,
}

// ---------------------------------------------------------------------------
// Generic envelopes
// ---------------------------------------------------------------------------

/// Generic error envelope returned to plugins.
#[derive(Debug, Serialize)]
pub(crate) struct HostError {
    pub error: String,
}

/// Generic success envelope (for void-returning operations).
#[derive(Debug, Serialize)]
pub(crate) struct HostOk {
    pub ok: bool,
}
