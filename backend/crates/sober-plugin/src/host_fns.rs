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
//! # Phase 1 vs Phase 2
//!
//! Phase 1 functions have the correct signatures and input/output types but
//! some return "not yet connected" stub errors because the backing services
//! (DB pool, tool registry, etc.) are not wired in yet.  `host_log` is
//! fully functional.  Phase 2 functions are pure stubs.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use extism::{CurrentPlugin, Function, PTR, UserData, Val};
use serde::{Deserialize, Serialize};
use sober_core::types::ids::PluginId;
use tracing::{debug, error, info, trace, warn};

use crate::capability::Capability;

// ---------------------------------------------------------------------------
// HostContext — shared state available to all host functions
// ---------------------------------------------------------------------------

/// Shared context passed to all host functions via Extism's `UserData` mechanism.
///
/// Carries the plugin identity and granted capabilities so that each host
/// function can enforce permission checks.  Future phases will add a DB
/// pool handle, tool registry reference, etc.
#[derive(Debug, Clone)]
pub struct HostContext {
    /// Identity of the plugin instance these functions belong to.
    pub plugin_id: PluginId,
    /// Capabilities granted to this plugin (resolved from its manifest).
    pub capabilities: Vec<Capability>,
    /// In-memory KV store for Phase 1 (replaced by DB-backed store later).
    pub kv_store: Arc<Mutex<HashMap<String, serde_json::Value>>>,
}

impl HostContext {
    /// Creates a new host context for the given plugin.
    pub fn new(plugin_id: PluginId, capabilities: Vec<Capability>) -> Self {
        Self {
            plugin_id,
            capabilities,
            kv_store: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Returns `true` if the plugin has been granted the given capability.
    fn has_capability(&self, check: &CapabilityKind) -> bool {
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
enum CapabilityKind {
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
struct LogRequest {
    level: String,
    message: String,
    #[serde(default)]
    fields: HashMap<String, serde_json::Value>,
}

/// Input for `host_kv_get`.
#[derive(Debug, Deserialize)]
struct KvGetRequest {
    key: String,
}

/// Output for `host_kv_get`.
#[derive(Debug, Serialize)]
struct KvGetResponse {
    value: Option<serde_json::Value>,
}

/// Input for `host_kv_set`.
#[derive(Debug, Deserialize)]
struct KvSetRequest {
    key: String,
    value: serde_json::Value,
}

// Stub request/response types: fields are deserialized to validate the
// contract but not yet read by stub implementations.  `#[allow(dead_code)]`
// silences warnings until the backing services are wired in.

/// Input for `host_http_request`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct HttpRequest {
    method: String,
    url: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body: Option<String>,
}

/// Output for `host_http_request`.
#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct HttpResponse {
    status: u16,
    #[serde(default)]
    headers: HashMap<String, String>,
    body: String,
}

/// Input for `host_read_secret`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ReadSecretRequest {
    name: String,
}

/// Output for `host_read_secret`.
#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct ReadSecretResponse {
    value: String,
}

/// Input for `host_call_tool`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CallToolRequest {
    tool: String,
    #[serde(default)]
    input: serde_json::Value,
}

/// Output for `host_call_tool`.
#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct CallToolResponse {
    output: serde_json::Value,
}

/// Input for `host_kv_delete`.
#[derive(Debug, Deserialize)]
struct KvDeleteRequest {
    key: String,
}

/// Input for `host_kv_list`.
#[derive(Debug, Deserialize)]
struct KvListRequest {
    #[serde(default)]
    prefix: Option<String>,
}

/// Output for `host_kv_list`.
#[derive(Debug, Serialize)]
struct KvListResponse {
    keys: Vec<String>,
}

/// Input for `host_emit_metric`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct EmitMetricRequest {
    name: String,
    kind: String,
    value: f64,
    #[serde(default)]
    labels: HashMap<String, String>,
}

/// Input for `host_memory_query`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MemoryQueryRequest {
    query: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

/// Input for `host_memory_write`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MemoryWriteRequest {
    content: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    metadata: HashMap<String, serde_json::Value>,
}

/// Input for `host_conversation_read`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ConversationReadRequest {
    conversation_id: String,
    #[serde(default)]
    limit: Option<u32>,
}

/// Input for `host_schedule`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ScheduleRequest {
    /// Cron expression or interval (e.g. "*/5 * * * *" or "30s").
    schedule: String,
    /// Payload to deliver when the job fires.
    payload: serde_json::Value,
}

/// Input for `host_fs_read`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FsReadRequest {
    path: String,
}

/// Input for `host_fs_write`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FsWriteRequest {
    path: String,
    content: String,
}

/// Input for `host_llm_complete`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LlmCompleteRequest {
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    max_tokens: Option<u32>,
}

/// Generic error envelope returned to plugins.
#[derive(Debug, Serialize)]
struct HostError {
    error: String,
}

/// Generic success envelope (for void-returning operations).
#[derive(Debug, Serialize)]
struct HostOk {
    ok: bool,
}

// ---------------------------------------------------------------------------
// Helper: read JSON input from WASM memory
// ---------------------------------------------------------------------------

/// Reads a JSON-encoded value from the plugin's memory at the given input offset.
fn read_input<T: serde::de::DeserializeOwned>(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
) -> Result<T, extism::Error> {
    let input: String = plugin.memory_get_val(&inputs[0])?;
    serde_json::from_str(&input).map_err(|e| extism::Error::msg(format!("invalid JSON input: {e}")))
}

/// Writes a JSON-encoded value to plugin memory and stores the handle in outputs.
fn write_output<T: Serialize>(
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
fn capability_denied_error(
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
fn not_yet_connected_error(
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
// Host function implementations
// ---------------------------------------------------------------------------

/// Structured logging from the plugin.
///
/// Always available — no capability gate required.  Maps plugin log levels
/// to `tracing` levels on the host side.
fn host_log_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: LogRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;
    let plugin_id = ctx.plugin_id;

    match req.level.to_lowercase().as_str() {
        "trace" => trace!(plugin_id = %plugin_id, fields = ?req.fields, "{}", req.message),
        "debug" => debug!(plugin_id = %plugin_id, fields = ?req.fields, "{}", req.message),
        "info" => info!(plugin_id = %plugin_id, fields = ?req.fields, "{}", req.message),
        "warn" => warn!(plugin_id = %plugin_id, fields = ?req.fields, "{}", req.message),
        "error" => error!(plugin_id = %plugin_id, fields = ?req.fields, "{}", req.message),
        _ => info!(plugin_id = %plugin_id, fields = ?req.fields, "[{}] {}", req.level, req.message),
    }

    let ok = HostOk { ok: true };
    write_output(plugin, outputs, &ok)
}

/// Reads a value from plugin-scoped key-value storage.
///
/// Requires the `KeyValue` capability.  Phase 1 uses an in-memory store.
fn host_kv_get_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: KvGetRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::KeyValue) {
        return capability_denied_error(plugin, outputs, "key_value");
    }

    let kv = ctx
        .kv_store
        .lock()
        .map_err(|e| extism::Error::msg(format!("kv lock poisoned: {e}")))?;
    let value = kv.get(&req.key).cloned();

    let resp = KvGetResponse { value };
    write_output(plugin, outputs, &resp)
}

/// Writes a value to plugin-scoped key-value storage.
///
/// Requires the `KeyValue` capability.  Phase 1 uses an in-memory store.
fn host_kv_set_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: KvSetRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::KeyValue) {
        return capability_denied_error(plugin, outputs, "key_value");
    }

    let mut kv = ctx
        .kv_store
        .lock()
        .map_err(|e| extism::Error::msg(format!("kv lock poisoned: {e}")))?;
    kv.insert(req.key, req.value);

    let ok = HostOk { ok: true };
    write_output(plugin, outputs, &ok)
}

/// Deletes a key from plugin-scoped key-value storage.
///
/// Requires the `KeyValue` capability.  Phase 1 uses an in-memory store.
fn host_kv_delete_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: KvDeleteRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::KeyValue) {
        return capability_denied_error(plugin, outputs, "key_value");
    }

    let mut kv = ctx
        .kv_store
        .lock()
        .map_err(|e| extism::Error::msg(format!("kv lock poisoned: {e}")))?;
    kv.remove(&req.key);

    let ok = HostOk { ok: true };
    write_output(plugin, outputs, &ok)
}

/// Lists keys in plugin-scoped key-value storage, optionally filtered by prefix.
///
/// Requires the `KeyValue` capability.  Phase 1 uses an in-memory store.
fn host_kv_list_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: KvListRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::KeyValue) {
        return capability_denied_error(plugin, outputs, "key_value");
    }

    let kv = ctx
        .kv_store
        .lock()
        .map_err(|e| extism::Error::msg(format!("kv lock poisoned: {e}")))?;

    let keys: Vec<String> = match &req.prefix {
        Some(prefix) => kv
            .keys()
            .filter(|k| k.starts_with(prefix.as_str()))
            .cloned()
            .collect(),
        None => kv.keys().cloned().collect(),
    };

    let resp = KvListResponse { keys };
    write_output(plugin, outputs, &resp)
}

/// Makes an outbound HTTP request.
///
/// Requires the `Network` capability.  Phase 1: returns a stub error.
fn host_http_request_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let _req: HttpRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::Network) {
        return capability_denied_error(plugin, outputs, "network");
    }

    not_yet_connected_error(plugin, outputs, "host_http_request")
}

/// Reads a secret from the vault.
///
/// Requires the `SecretRead` capability.  Phase 1: returns a stub error.
fn host_read_secret_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let _req: ReadSecretRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::SecretRead) {
        return capability_denied_error(plugin, outputs, "secret_read");
    }

    not_yet_connected_error(plugin, outputs, "host_read_secret")
}

/// Calls another tool/plugin.
///
/// Requires the `ToolCall` capability.  Phase 1: returns a stub error.
fn host_call_tool_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let _req: CallToolRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::ToolCall) {
        return capability_denied_error(plugin, outputs, "tool_call");
    }

    not_yet_connected_error(plugin, outputs, "host_call_tool")
}

/// Emits a metric (counter, gauge, or histogram sample).
///
/// Requires the `Metrics` capability.  Phase 1: returns a stub error.
fn host_emit_metric_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let _req: EmitMetricRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::Metrics) {
        return capability_denied_error(plugin, outputs, "metrics");
    }

    not_yet_connected_error(plugin, outputs, "host_emit_metric")
}

// ---------------------------------------------------------------------------
// Phase 2+ stubs
// ---------------------------------------------------------------------------

/// Queries the memory/context system.
///
/// Requires the `MemoryRead` capability.  Phase 2: returns a stub error.
fn host_memory_query_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let _req: MemoryQueryRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::MemoryRead) {
        return capability_denied_error(plugin, outputs, "memory_read");
    }

    not_yet_connected_error(plugin, outputs, "host_memory_query")
}

/// Writes to the memory/context system.
///
/// Requires the `MemoryWrite` capability.  Phase 2: returns a stub error.
fn host_memory_write_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let _req: MemoryWriteRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::MemoryWrite) {
        return capability_denied_error(plugin, outputs, "memory_write");
    }

    not_yet_connected_error(plugin, outputs, "host_memory_write")
}

/// Reads conversation history.
///
/// Requires the `ConversationRead` capability.  Phase 2: returns a stub error.
fn host_conversation_read_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let _req: ConversationReadRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::ConversationRead) {
        return capability_denied_error(plugin, outputs, "conversation_read");
    }

    not_yet_connected_error(plugin, outputs, "host_conversation_read")
}

/// Creates or manages a scheduled job.
///
/// Requires the `Schedule` capability.  Phase 2: returns a stub error.
fn host_schedule_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let _req: ScheduleRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::Schedule) {
        return capability_denied_error(plugin, outputs, "schedule");
    }

    not_yet_connected_error(plugin, outputs, "host_schedule")
}

/// Reads a file from the sandboxed filesystem.
///
/// Requires the `Filesystem` capability.  Phase 2: returns a stub error.
fn host_fs_read_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let _req: FsReadRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::Filesystem) {
        return capability_denied_error(plugin, outputs, "filesystem");
    }

    not_yet_connected_error(plugin, outputs, "host_fs_read")
}

/// Writes a file to the sandboxed filesystem.
///
/// Requires the `Filesystem` capability.  Phase 2: returns a stub error.
fn host_fs_write_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let _req: FsWriteRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::Filesystem) {
        return capability_denied_error(plugin, outputs, "filesystem");
    }

    not_yet_connected_error(plugin, outputs, "host_fs_write")
}

/// Sends a prompt to an LLM provider.
///
/// Requires the `LlmCall` capability.  Phase 2: returns a stub error.
fn host_llm_complete_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let _req: LlmCompleteRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::LlmCall) {
        return capability_denied_error(plugin, outputs, "llm_call");
    }

    not_yet_connected_error(plugin, outputs, "host_llm_complete")
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
        // Phase 1 — KeyValue (functional with in-memory store)
        ("host_kv_get", host_kv_get_impl as HostFn),
        ("host_kv_set", host_kv_set_impl as HostFn),
        ("host_kv_delete", host_kv_delete_impl as HostFn),
        ("host_kv_list", host_kv_list_impl as HostFn),
        // Phase 1 — stubs
        ("host_http_request", host_http_request_impl as HostFn),
        ("host_read_secret", host_read_secret_impl as HostFn),
        ("host_call_tool", host_call_tool_impl as HostFn),
        ("host_emit_metric", host_emit_metric_impl as HostFn),
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

    #[test]
    fn kv_store_shared_between_get_and_set() {
        let ctx = test_context(vec![Capability::KeyValue]);

        // Insert via the shared store directly to verify structure
        {
            let kv = ctx.kv_store.lock().expect("lock");
            assert!(kv.is_empty());
        }

        // Insert a value
        {
            let mut kv = ctx.kv_store.lock().expect("lock");
            kv.insert("test_key".into(), serde_json::json!("test_value"));
        }

        // Verify it's readable
        {
            let kv = ctx.kv_store.lock().expect("lock");
            assert_eq!(kv.get("test_key"), Some(&serde_json::json!("test_value")));
        }
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
}
