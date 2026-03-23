//! Tool execution backend trait.
//!
//! [`ToolExecutor`] provides an object-safe interface for cross-tool
//! invocation from within a plugin.  The `depth` parameter enables
//! recursion guarding — when a plugin calls a tool that calls another
//! plugin, depth increments.

use std::future::Future;
use std::pin::Pin;

/// Object-safe backend for executing named tools.
///
/// Implementations should reject calls when `depth` exceeds a configured
/// maximum (e.g. 5) to prevent infinite recursion between plugins.
pub trait ToolExecutor: Send + Sync {
    /// Executes a named tool with the given input. Returns the output as a string.
    fn execute(
        &self,
        tool_name: &str,
        input: serde_json::Value,
        depth: u32,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;
}

// ---------------------------------------------------------------------------
// Compile-time assertions
// ---------------------------------------------------------------------------

// ToolExecutor is object-safe and dyn-compatible.
#[allow(dead_code)]
const _: () = {
    fn assert_object_safe(_: &dyn ToolExecutor) {}
};

// Arc<dyn ToolExecutor> is Send + Sync.
#[allow(dead_code)]
const _: () = {
    fn assert_send_sync<T: Send + Sync>() {}
    fn check() {
        assert_send_sync::<std::sync::Arc<dyn ToolExecutor>>();
    }
};
