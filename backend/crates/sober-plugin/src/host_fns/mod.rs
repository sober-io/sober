//! Host function definitions for the WASM plugin runtime.
//!
//! Each host function is registered with Extism when a plugin instance is
//! created.  Functions use JSON serialization across the WASM boundary:
//! the plugin sends a JSON request, the host deserializes it, performs the
//! operation (or returns a stub error), and serializes the result back.
//!
//! # Capability gating
//!
//! Most host functions require a specific [`Capability`].  The
//! [`HostContext`] carries the granted capabilities so each function can
//! check before executing.  `host_log` is the only function that is always
//! available.
//!
//! # Functional host functions
//!
//! The following host functions are fully operational:
//!
//! - `host_log` — structured logging (always available, no capability gate)
//! - `host_kv_*` — plugin-scoped key-value storage (via `KvBackend` trait, `KeyValue` capability)
//! - `host_http_request` — outbound HTTP via `ureq` (`Network` capability, domain-restricted)
//! - `host_emit_metric` — counter/gauge/histogram emission via `metrics` crate (`Metrics` capability)
//! - `host_fs_read` — sandboxed file read via `std::fs` (`Filesystem` capability, path-restricted)
//! - `host_fs_write` — sandboxed file write via `std::fs` (`Filesystem` capability, path-restricted)
//!
//! Remaining host functions (secrets, tool calls, memory, LLM,
//! scheduling, conversation) return "not yet connected" stub errors until
//! the backing services are wired in.

mod conversation;
mod filesystem;
mod kv;
mod llm;
mod log;
mod memory;
mod metrics;
mod network;
mod schedule;
mod secrets;
mod tool_call;

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use extism::{CurrentPlugin, Function, PTR, UserData, Val};
use serde::{Deserialize, Serialize};
use sober_core::types::ids::{PluginId, UserId};

use crate::backends::InMemoryKvBackend;
use crate::backends::KvBackend;
use crate::backends::{
    ConversationBackend, MemoryBackend, ScheduleBackend, SecretBackend, ToolExecutor,
};
use crate::capability::Capability;

use self::conversation::host_conversation_read_impl;
use self::filesystem::{host_fs_read_impl, host_fs_write_impl};
use self::kv::{host_kv_delete_impl, host_kv_get_impl, host_kv_list_impl, host_kv_set_impl};
use self::llm::host_llm_complete_impl;
use self::log::host_log_impl;
use self::memory::{host_memory_query_impl, host_memory_write_impl};
use self::metrics::host_emit_metric_impl;
use self::network::host_http_request_impl;
use self::schedule::host_schedule_impl;
use self::secrets::host_read_secret_impl;
use self::tool_call::host_call_tool_impl;

// ---------------------------------------------------------------------------
// HostContext — shared state available to all host functions
// ---------------------------------------------------------------------------

/// Shared context passed to all host functions via Extism's `UserData` mechanism.
///
/// Carries the plugin identity and granted capabilities so that each host
/// function can enforce permission checks.  The optional `runtime_handle`
/// enables host functions to bridge into async code (e.g. calling services
/// that require a tokio runtime).
#[derive(Clone)]
pub struct HostContext {
    /// Identity of the plugin instance these functions belong to.
    pub plugin_id: PluginId,
    /// Capabilities granted to this plugin (resolved from its manifest).
    pub capabilities: Vec<Capability>,
    /// Backend for plugin-scoped key-value storage.
    pub kv_backend: Arc<dyn KvBackend>,
    /// Tokio runtime handle for bridging async operations from synchronous
    /// host function calls.  `None` in test mode or when no runtime is available.
    pub runtime_handle: Option<tokio::runtime::Handle>,
    /// User ID for scoped operations (memory, conversation, etc.).
    /// `None` when the plugin is invoked outside a user context (e.g. system jobs).
    pub user_id: Option<UserId>,
    /// LLM engine for completion requests.  `None` when no LLM provider is configured.
    pub llm_engine: Option<Arc<dyn sober_llm::LlmEngine>>,
    /// Secret reading backend.  `None` when vault is not configured.
    pub secret_backend: Option<Arc<dyn SecretBackend>>,
    /// Memory read/write backend.  `None` when memory store is not configured.
    pub memory_backend: Option<Arc<dyn MemoryBackend>>,
    /// Conversation reading backend.  `None` when message store is not configured.
    pub conversation_backend: Option<Arc<dyn ConversationBackend>>,
    /// Job scheduling backend.  `None` when scheduler is not connected.
    pub schedule_backend: Option<Arc<dyn ScheduleBackend>>,
    /// Tool execution backend.  `None` when tool registry is not available.
    pub tool_executor: Option<Arc<dyn ToolExecutor>>,
}

impl fmt::Debug for HostContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostContext")
            .field("plugin_id", &self.plugin_id)
            .field("capabilities", &self.capabilities)
            .field("kv_backend", &"<dyn KvBackend>")
            .field("runtime_handle", &self.runtime_handle.is_some())
            .field("user_id", &self.user_id)
            .field(
                "llm_engine",
                if self.llm_engine.is_some() {
                    &"<LlmEngine>"
                } else {
                    &"<none>"
                },
            )
            .field("secret_backend", &self.secret_backend.is_some())
            .field("memory_backend", &self.memory_backend.is_some())
            .field("conversation_backend", &self.conversation_backend.is_some())
            .field("schedule_backend", &self.schedule_backend.is_some())
            .field("tool_executor", &self.tool_executor.is_some())
            .finish()
    }
}

impl HostContext {
    /// Creates a new host context for the given plugin.
    ///
    /// Defaults to [`InMemoryKvBackend`] for KV storage.  Use
    /// [`with_kv_backend`](Self::with_kv_backend) to supply a
    /// database-backed implementation.
    pub fn new(plugin_id: PluginId, capabilities: Vec<Capability>) -> Self {
        Self {
            plugin_id,
            capabilities,
            kv_backend: Arc::new(InMemoryKvBackend::new()),
            runtime_handle: None,
            user_id: None,
            llm_engine: None,
            secret_backend: None,
            memory_backend: None,
            conversation_backend: None,
            schedule_backend: None,
            tool_executor: None,
        }
    }

    /// Sets the KV backend for plugin-scoped key-value storage.
    #[must_use]
    pub fn with_kv_backend(mut self, backend: Arc<dyn KvBackend>) -> Self {
        self.kv_backend = backend;
        self
    }

    /// Sets the tokio runtime handle for async bridging.
    #[must_use]
    pub fn with_runtime_handle(mut self, handle: tokio::runtime::Handle) -> Self {
        self.runtime_handle = Some(handle);
        self
    }

    /// Sets the user ID for scoped operations.
    #[must_use]
    pub fn with_user_id(mut self, user_id: UserId) -> Self {
        self.user_id = Some(user_id);
        self
    }

    /// Sets the LLM engine for completion requests.
    #[must_use]
    pub fn with_llm_engine(mut self, engine: Arc<dyn sober_llm::LlmEngine>) -> Self {
        self.llm_engine = Some(engine);
        self
    }

    /// Sets the secret reading backend.
    #[must_use]
    pub fn with_secret_backend(mut self, backend: Arc<dyn SecretBackend>) -> Self {
        self.secret_backend = Some(backend);
        self
    }

    /// Sets the memory read/write backend.
    #[must_use]
    pub fn with_memory_backend(mut self, backend: Arc<dyn MemoryBackend>) -> Self {
        self.memory_backend = Some(backend);
        self
    }

    /// Sets the conversation reading backend.
    #[must_use]
    pub fn with_conversation_backend(mut self, backend: Arc<dyn ConversationBackend>) -> Self {
        self.conversation_backend = Some(backend);
        self
    }

    /// Sets the job scheduling backend.
    #[must_use]
    pub fn with_schedule_backend(mut self, backend: Arc<dyn ScheduleBackend>) -> Self {
        self.schedule_backend = Some(backend);
        self
    }

    /// Sets the tool execution backend.
    #[must_use]
    pub fn with_tool_executor(mut self, executor: Arc<dyn ToolExecutor>) -> Self {
        self.tool_executor = Some(executor);
        self
    }

    /// Runs an async future synchronously using the stored runtime handle.
    ///
    /// Returns an error if no runtime handle is available (e.g. in test mode).
    pub fn block_on_async<F: std::future::Future>(&self, f: F) -> Result<F::Output, extism::Error> {
        let handle = self
            .runtime_handle
            .as_ref()
            .ok_or_else(|| extism::Error::msg("no runtime handle available"))?;
        Ok(handle.block_on(f))
    }

    /// Returns `true` if the plugin has been granted the given capability.
    pub(crate) fn has_capability(&self, check: &CapabilityKind) -> bool {
        self.capabilities.iter().any(|c| {
            matches!(
                (check, c),
                (CapabilityKind::KeyValue, Capability::KeyValue)
                    | (CapabilityKind::Network, Capability::Network { .. })
                    | (CapabilityKind::SecretRead, Capability::SecretRead)
                    | (CapabilityKind::ToolCall, Capability::ToolCall { .. })
                    | (CapabilityKind::Metrics, Capability::Metrics)
                    | (CapabilityKind::MemoryRead, Capability::MemoryRead { .. })
                    | (CapabilityKind::MemoryWrite, Capability::MemoryWrite { .. })
                    | (
                        CapabilityKind::ConversationRead,
                        Capability::ConversationRead
                    )
                    | (CapabilityKind::Schedule, Capability::Schedule)
                    | (CapabilityKind::Filesystem, Capability::Filesystem { .. })
                    | (CapabilityKind::LlmCall, Capability::LlmCall)
            )
        })
    }
}

/// Simplified capability kind used for permission checks (no restriction data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CapabilityKind {
    KeyValue,
    Network,
    SecretRead,
    ToolCall,
    Metrics,
    MemoryRead,
    MemoryWrite,
    ConversationRead,
    Schedule,
    Filesystem,
    LlmCall,
}

// ---------------------------------------------------------------------------
// JSON request / response types for the WASM boundary
// ---------------------------------------------------------------------------

/// Input for `host_log`.
#[derive(Debug, Deserialize)]
pub(crate) struct LogRequest {
    pub level: String,
    pub message: String,
    #[serde(default)]
    pub fields: HashMap<String, serde_json::Value>,
}

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

// Stub request/response types: fields are deserialized to validate the
// contract but not yet read by stub implementations.  `#[allow(dead_code)]`
// silences warnings until the backing services are wired in.

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

/// Input for `host_read_secret`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // fields read via serde; will be used when backend is wired
pub(crate) struct ReadSecretRequest {
    pub name: String,
}

/// Output for `host_read_secret`.
#[derive(Debug, Serialize)]
#[allow(dead_code)] // will be constructed when backend is wired
pub(crate) struct ReadSecretResponse {
    pub value: String,
}

/// Input for `host_call_tool`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // fields read via serde; will be used when backend is wired
pub(crate) struct CallToolRequest {
    pub tool: String,
    #[serde(default)]
    pub input: serde_json::Value,
}

/// Output for `host_call_tool`.
#[derive(Debug, Serialize)]
#[allow(dead_code)] // will be constructed when backend is wired
pub(crate) struct CallToolResponse {
    pub output: serde_json::Value,
}

/// Output for `host_memory_query`.
#[derive(Debug, Serialize)]
#[allow(dead_code)] // will be constructed when backend is wired
pub(crate) struct MemoryQueryResponse {
    pub results: Vec<crate::backends::MemoryHit>,
}

/// Output for `host_conversation_read`.
#[derive(Debug, Serialize)]
#[allow(dead_code)] // will be constructed when backend is wired
pub(crate) struct ConversationReadResponse {
    pub messages: Vec<crate::backends::ConversationMessage>,
}

/// Output for `host_schedule`.
#[derive(Debug, Serialize)]
#[allow(dead_code)] // will be constructed when backend is wired
pub(crate) struct ScheduleResponse {
    pub job_id: String,
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

/// Input for `host_emit_metric`.
#[derive(Debug, Deserialize)]
pub(crate) struct EmitMetricRequest {
    pub name: String,
    pub kind: String,
    pub value: f64,
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

/// Input for `host_memory_query`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // fields read via serde; will be used when backend is wired
pub(crate) struct MemoryQueryRequest {
    pub query: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Input for `host_memory_write`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // fields read via serde; will be used when backend is wired
pub(crate) struct MemoryWriteRequest {
    pub content: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Input for `host_conversation_read`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // fields read via serde; will be used when backend is wired
pub(crate) struct ConversationReadRequest {
    pub conversation_id: String,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Input for `host_schedule`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // fields read via serde; will be used when backend is wired
pub(crate) struct ScheduleRequest {
    /// Cron expression or interval (e.g. "*/5 * * * *" or "30s").
    pub schedule: String,
    /// Payload to deliver when the job fires.
    pub payload: serde_json::Value,
}

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

// ---------------------------------------------------------------------------
// Helper: read JSON input from WASM memory
// ---------------------------------------------------------------------------

/// Reads a JSON-encoded value from the plugin's memory at the given input offset.
pub(crate) fn read_input<T: serde::de::DeserializeOwned>(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
) -> Result<T, extism::Error> {
    let input: String = plugin.memory_get_val(&inputs[0])?;
    serde_json::from_str(&input).map_err(|e| extism::Error::msg(format!("invalid JSON input: {e}")))
}

/// Writes a JSON-encoded value to plugin memory and stores the handle in outputs.
pub(crate) fn write_output<T: Serialize>(
    plugin: &mut CurrentPlugin,
    outputs: &mut [Val],
    value: &T,
) -> Result<(), extism::Error> {
    let json = serde_json::to_string(value)
        .map_err(|e| extism::Error::msg(format!("failed to serialize output: {e}")))?;
    let handle = plugin.memory_new(&json)?;
    if !outputs.is_empty() {
        outputs[0] = plugin.memory_to_val(handle);
    }
    Ok(())
}

/// Returns an error response to the plugin for a denied capability.
pub(crate) fn capability_denied_error(
    plugin: &mut CurrentPlugin,
    outputs: &mut [Val],
    capability: &str,
) -> Result<(), extism::Error> {
    let err = HostError {
        error: format!("capability denied: {capability}"),
    };
    write_output(plugin, outputs, &err)
}

/// Returns a "not yet connected" stub error to the plugin.
pub(crate) fn not_yet_connected_error(
    plugin: &mut CurrentPlugin,
    outputs: &mut [Val],
    function_name: &str,
) -> Result<(), extism::Error> {
    let err = HostError {
        error: format!("{function_name}: not yet connected (stub)"),
    };
    write_output(plugin, outputs, &err)
}

// ---------------------------------------------------------------------------
// Public API: build the full set of host functions
// ---------------------------------------------------------------------------

/// Namespace for all Sober host functions in the WASM module.
pub const HOST_NAMESPACE: &str = "sober";

/// Builds the complete set of host functions for a plugin instance.
///
/// The returned [`Vec<Function>`] should be passed to the Extism
/// `PluginBuilder` when creating the plugin.  Each function is namespaced
/// under [`HOST_NAMESPACE`] (`"sober"`).
///
/// # Arguments
///
/// * `ctx` - Shared context carrying plugin identity and granted capabilities.
///
/// # Example
///
/// ```ignore
/// let ctx = HostContext::new(plugin_id, capabilities);
/// let functions = build_host_functions(ctx);
/// // Pass `functions` to PluginBuilder::with_functions(...)
/// ```
pub fn build_host_functions(ctx: HostContext) -> Vec<Function> {
    let user_data = UserData::new(ctx);

    let functions = vec![
        // Always available
        ("host_log", host_log_impl as HostFn),
        // Phase 1 — KeyValue (via KvBackend trait)
        ("host_kv_get", host_kv_get_impl as HostFn),
        ("host_kv_set", host_kv_set_impl as HostFn),
        ("host_kv_delete", host_kv_delete_impl as HostFn),
        ("host_kv_list", host_kv_list_impl as HostFn),
        // Phase 1 — functional (network + metrics)
        ("host_http_request", host_http_request_impl as HostFn),
        ("host_emit_metric", host_emit_metric_impl as HostFn),
        // Phase 1 — stubs (secrets, tool calls)
        ("host_read_secret", host_read_secret_impl as HostFn),
        ("host_call_tool", host_call_tool_impl as HostFn),
        // Phase 2+ — stubs
        ("host_memory_query", host_memory_query_impl as HostFn),
        ("host_memory_write", host_memory_write_impl as HostFn),
        (
            "host_conversation_read",
            host_conversation_read_impl as HostFn,
        ),
        ("host_schedule", host_schedule_impl as HostFn),
        ("host_fs_read", host_fs_read_impl as HostFn),
        ("host_fs_write", host_fs_write_impl as HostFn),
        ("host_llm_complete", host_llm_complete_impl as HostFn),
    ];

    functions
        .into_iter()
        .map(|(name, f)| {
            Function::new(
                name,
                [PTR], // single input: JSON-encoded request
                [PTR], // single output: JSON-encoded response
                user_data.clone(),
                f,
            )
            .with_namespace(HOST_NAMESPACE)
        })
        .collect()
}

/// Type alias for host function signatures used in registration.
type HostFn =
    fn(&mut CurrentPlugin, &[Val], &mut [Val], UserData<HostContext>) -> Result<(), extism::Error>;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;

    fn test_context(capabilities: Vec<Capability>) -> HostContext {
        HostContext::new(PluginId::new(), capabilities)
    }

    #[test]
    fn host_context_has_capability() {
        let ctx = test_context(vec![
            Capability::KeyValue,
            Capability::Network {
                domains: vec!["example.com".into()],
            },
        ]);

        assert!(ctx.has_capability(&CapabilityKind::KeyValue));
        assert!(ctx.has_capability(&CapabilityKind::Network));
        assert!(!ctx.has_capability(&CapabilityKind::SecretRead));
        assert!(!ctx.has_capability(&CapabilityKind::ToolCall));
        assert!(!ctx.has_capability(&CapabilityKind::Metrics));
    }

    #[test]
    fn host_context_no_capabilities() {
        let ctx = test_context(vec![]);

        assert!(!ctx.has_capability(&CapabilityKind::KeyValue));
        assert!(!ctx.has_capability(&CapabilityKind::Network));
        assert!(!ctx.has_capability(&CapabilityKind::MemoryRead));
        assert!(!ctx.has_capability(&CapabilityKind::LlmCall));
    }

    #[test]
    fn host_context_all_capabilities() {
        let ctx = test_context(vec![
            Capability::KeyValue,
            Capability::Network { domains: vec![] },
            Capability::SecretRead,
            Capability::ToolCall { tools: vec![] },
            Capability::Metrics,
            Capability::MemoryRead { scopes: vec![] },
            Capability::MemoryWrite { scopes: vec![] },
            Capability::ConversationRead,
            Capability::Schedule,
            Capability::Filesystem { paths: vec![] },
            Capability::LlmCall,
        ]);

        assert!(ctx.has_capability(&CapabilityKind::KeyValue));
        assert!(ctx.has_capability(&CapabilityKind::Network));
        assert!(ctx.has_capability(&CapabilityKind::SecretRead));
        assert!(ctx.has_capability(&CapabilityKind::ToolCall));
        assert!(ctx.has_capability(&CapabilityKind::Metrics));
        assert!(ctx.has_capability(&CapabilityKind::MemoryRead));
        assert!(ctx.has_capability(&CapabilityKind::MemoryWrite));
        assert!(ctx.has_capability(&CapabilityKind::ConversationRead));
        assert!(ctx.has_capability(&CapabilityKind::Schedule));
        assert!(ctx.has_capability(&CapabilityKind::Filesystem));
        assert!(ctx.has_capability(&CapabilityKind::LlmCall));
    }

    #[test]
    fn build_host_functions_returns_all() {
        let ctx = test_context(vec![]);
        let fns = build_host_functions(ctx);

        // 16 total host functions
        assert_eq!(fns.len(), 16);

        let names: Vec<&str> = fns.iter().map(|f| f.name()).collect();
        assert!(names.contains(&"host_log"));
        assert!(names.contains(&"host_kv_get"));
        assert!(names.contains(&"host_kv_set"));
        assert!(names.contains(&"host_kv_delete"));
        assert!(names.contains(&"host_kv_list"));
        assert!(names.contains(&"host_http_request"));
        assert!(names.contains(&"host_read_secret"));
        assert!(names.contains(&"host_call_tool"));
        assert!(names.contains(&"host_emit_metric"));
        assert!(names.contains(&"host_memory_query"));
        assert!(names.contains(&"host_memory_write"));
        assert!(names.contains(&"host_conversation_read"));
        assert!(names.contains(&"host_schedule"));
        assert!(names.contains(&"host_fs_read"));
        assert!(names.contains(&"host_fs_write"));
        assert!(names.contains(&"host_llm_complete"));
    }

    #[test]
    fn all_functions_have_correct_namespace() {
        let ctx = test_context(vec![]);
        let fns = build_host_functions(ctx);

        for f in &fns {
            assert_eq!(
                f.namespace(),
                Some(HOST_NAMESPACE),
                "function {} has wrong namespace",
                f.name()
            );
        }
    }

    #[test]
    fn all_functions_have_ptr_signature() {
        let ctx = test_context(vec![]);
        let fns = build_host_functions(ctx);

        for f in &fns {
            assert_eq!(
                f.params(),
                &[PTR],
                "function {} should have [PTR] params",
                f.name()
            );
            assert_eq!(
                f.results(),
                &[PTR],
                "function {} should have [PTR] results",
                f.name()
            );
        }
    }

    #[tokio::test]
    async fn kv_backend_shared_between_get_and_set() {
        let ctx = test_context(vec![Capability::KeyValue]);
        let backend = Arc::clone(&ctx.kv_backend);
        let pid = ctx.plugin_id;

        // Initially empty
        let val = backend.get(pid, "test_key").await.expect("get");
        assert!(val.is_none());

        // Insert a value
        backend
            .set(pid, "test_key", serde_json::json!("test_value"))
            .await
            .expect("set");

        // Verify it's readable
        let val = backend.get(pid, "test_key").await.expect("get");
        assert_eq!(val, Some(serde_json::json!("test_value")));
    }

    #[test]
    fn log_request_deserializes_minimal() {
        let json = r#"{"level": "info", "message": "hello"}"#;
        let req: LogRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.level, "info");
        assert_eq!(req.message, "hello");
        assert!(req.fields.is_empty());
    }

    #[test]
    fn log_request_deserializes_with_fields() {
        let json =
            r#"{"level": "debug", "message": "test", "fields": {"key": "value", "count": 42}}"#;
        let req: LogRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.level, "debug");
        assert_eq!(req.fields.len(), 2);
        assert_eq!(req.fields.get("key"), Some(&serde_json::json!("value")));
    }

    #[test]
    fn kv_get_request_deserializes() {
        let json = r#"{"key": "my_key"}"#;
        let req: KvGetRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.key, "my_key");
    }

    #[test]
    fn kv_set_request_deserializes() {
        let json = r#"{"key": "my_key", "value": {"nested": true}}"#;
        let req: KvSetRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.key, "my_key");
        assert_eq!(req.value, serde_json::json!({"nested": true}));
    }

    #[test]
    fn kv_delete_request_deserializes() {
        let json = r#"{"key": "my_key"}"#;
        let req: KvDeleteRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.key, "my_key");
    }

    #[test]
    fn kv_list_request_deserializes_with_prefix() {
        let json = r#"{"prefix": "user:"}"#;
        let req: KvListRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.prefix, Some("user:".into()));
    }

    #[test]
    fn kv_list_request_deserializes_without_prefix() {
        let json = r#"{}"#;
        let req: KvListRequest = serde_json::from_str(json).expect("deserialize");
        assert!(req.prefix.is_none());
    }

    #[test]
    fn kv_list_response_serializes() {
        let resp = KvListResponse {
            keys: vec!["a".into(), "b".into()],
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains(r#""keys":["a","b"]"#));
    }

    #[test]
    fn http_request_deserializes_minimal() {
        let json = r#"{"method": "GET", "url": "https://example.com"}"#;
        let req: HttpRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.method, "GET");
        assert_eq!(req.url, "https://example.com");
        assert!(req.headers.is_empty());
        assert!(req.body.is_none());
    }

    #[test]
    fn call_tool_request_deserializes() {
        let json = r#"{"tool": "search", "input": {"query": "hello"}}"#;
        let req: CallToolRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.tool, "search");
        assert_eq!(req.input, serde_json::json!({"query": "hello"}));
    }

    #[test]
    fn emit_metric_request_deserializes() {
        let json = r#"{"name": "requests_total", "kind": "counter", "value": 1.0, "labels": {"method": "GET"}}"#;
        let req: EmitMetricRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.name, "requests_total");
        assert_eq!(req.kind, "counter");
        assert!((req.value - 1.0).abs() < f64::EPSILON);
        assert_eq!(req.labels.get("method"), Some(&"GET".to_string()));
    }

    #[test]
    fn host_error_serializes() {
        let err = HostError {
            error: "something went wrong".into(),
        };
        let json = serde_json::to_string(&err).expect("serialize");
        assert!(json.contains("something went wrong"));
    }

    #[test]
    fn host_ok_serializes() {
        let ok = HostOk { ok: true };
        let json = serde_json::to_string(&ok).expect("serialize");
        assert_eq!(json, r#"{"ok":true}"#);
    }

    #[test]
    fn kv_get_response_serializes_some() {
        let resp = KvGetResponse {
            value: Some(serde_json::json!("hello")),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains(r#""value":"hello""#));
    }

    #[test]
    fn kv_get_response_serializes_none() {
        let resp = KvGetResponse { value: None };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains(r#""value":null"#));
    }

    #[test]
    fn memory_query_request_deserializes() {
        let json = r#"{"query": "what is rust?", "scope": "user", "limit": 10}"#;
        let req: MemoryQueryRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.query, "what is rust?");
        assert_eq!(req.scope, Some("user".into()));
        assert_eq!(req.limit, Some(10));
    }

    #[test]
    fn schedule_request_deserializes() {
        let json = r#"{"schedule": "*/5 * * * *", "payload": {"action": "ping"}}"#;
        let req: ScheduleRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.schedule, "*/5 * * * *");
        assert_eq!(req.payload, serde_json::json!({"action": "ping"}));
    }

    #[test]
    fn llm_complete_request_deserializes() {
        let json = r#"{"prompt": "Hello", "model": "claude-3", "max_tokens": 100}"#;
        let req: LlmCompleteRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.prompt, "Hello");
        assert_eq!(req.model, Some("claude-3".into()));
        assert_eq!(req.max_tokens, Some(100));
    }

    #[test]
    fn fs_read_request_deserializes() {
        let json = r#"{"path": "/data/config.json"}"#;
        let req: FsReadRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.path, "/data/config.json");
    }

    #[test]
    fn fs_write_request_deserializes() {
        let json = r#"{"path": "/data/output.txt", "content": "hello world"}"#;
        let req: FsWriteRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.path, "/data/output.txt");
        assert_eq!(req.content, "hello world");
    }

    // -- extract_host tests --------------------------------------------------

    #[test]
    fn extract_host_https() {
        assert_eq!(
            network::extract_host("https://example.com/path"),
            Some("example.com".into())
        );
    }

    #[test]
    fn extract_host_http_with_port() {
        assert_eq!(
            network::extract_host("http://localhost:8080/api"),
            Some("localhost".into())
        );
    }

    #[test]
    fn extract_host_no_path() {
        assert_eq!(
            network::extract_host("https://api.example.com"),
            Some("api.example.com".into())
        );
    }

    #[test]
    fn extract_host_with_query() {
        assert_eq!(
            network::extract_host("https://example.com?q=1"),
            Some("example.com".into())
        );
    }

    #[test]
    fn extract_host_with_fragment() {
        assert_eq!(
            network::extract_host("https://example.com#section"),
            Some("example.com".into())
        );
    }

    #[test]
    fn extract_host_with_userinfo() {
        assert_eq!(
            network::extract_host("https://user:pass@example.com/path"),
            Some("example.com".into())
        );
    }

    #[test]
    fn extract_host_ipv6() {
        assert_eq!(
            network::extract_host("http://[::1]:8080/path"),
            Some("::1".into())
        );
    }

    #[test]
    fn extract_host_normalizes_case() {
        assert_eq!(
            network::extract_host("https://EXAMPLE.COM/path"),
            Some("example.com".into())
        );
    }

    #[test]
    fn extract_host_empty_returns_none() {
        assert_eq!(network::extract_host(""), None);
    }

    // -- HttpResponse serialization ------------------------------------------

    #[test]
    fn http_response_serializes() {
        let resp = HttpResponse {
            status: 200,
            headers: HashMap::new(),
            body: "ok".into(),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains(r#""status":200"#));
        assert!(json.contains(r#""body":"ok""#));
    }

    // -- EmitMetricRequest with empty labels ---------------------------------

    #[test]
    fn emit_metric_request_deserializes_no_labels() {
        let json = r#"{"name": "latency", "kind": "histogram", "value": 0.42}"#;
        let req: EmitMetricRequest = serde_json::from_str(json).expect("deserialize");
        assert_eq!(req.name, "latency");
        assert_eq!(req.kind, "histogram");
        assert!((req.value - 0.42).abs() < f64::EPSILON);
        assert!(req.labels.is_empty());
    }

    // -- New HostContext fields tests ----------------------------------------

    #[test]
    fn host_context_new_defaults() {
        let ctx = test_context(vec![]);
        // kv_backend defaults to InMemoryKvBackend (always present)
        assert!(ctx.runtime_handle.is_none());
        assert!(ctx.user_id.is_none());
    }

    #[test]
    fn host_context_with_user_id() {
        let user_id = UserId::new();
        let ctx = test_context(vec![]).with_user_id(user_id);
        assert_eq!(ctx.user_id, Some(user_id));
    }

    #[test]
    fn host_context_with_runtime_handle() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        let handle = rt.handle().clone();

        let ctx = test_context(vec![]).with_runtime_handle(handle);
        assert!(ctx.runtime_handle.is_some());
    }

    #[test]
    fn host_context_builder_chain() {
        let user_id = UserId::new();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        let handle = rt.handle().clone();

        let ctx = test_context(vec![])
            .with_runtime_handle(handle)
            .with_user_id(user_id);

        assert!(ctx.runtime_handle.is_some());
        assert_eq!(ctx.user_id, Some(user_id));
    }

    #[test]
    fn block_on_async_without_handle_returns_error() {
        let ctx = test_context(vec![]);
        let result = ctx.block_on_async(async { 42 });
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no runtime handle"),
        );
    }

    #[test]
    fn block_on_async_with_handle_runs_future() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        let handle = rt.handle().clone();

        let ctx = test_context(vec![]).with_runtime_handle(handle);
        let result = ctx.block_on_async(async { 42 });
        assert_eq!(result.expect("should succeed"), 42);
    }

    // Compile-time assertion that HostContext remains Send + Sync.
    #[allow(dead_code)]
    const _: () = {
        fn assert_send_sync<T: Send + Sync>() {}
        fn check() {
            assert_send_sync::<HostContext>();
        }
    };
}
