//! Host function: cross-tool invocation.

use std::sync::Arc;

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CallToolRequest, CallToolResponse, CapabilityKind, HostContext, capability_denied_error,
    not_yet_connected_error, read_input, write_output,
};
use crate::capability::Capability;

/// Calls another tool/plugin.
///
/// Requires the `ToolCall` capability.  If the capability carries a non-empty
/// `tools` list, only those tools may be invoked.
pub(crate) fn host_call_tool_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: CallToolRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::ToolCall) {
        return capability_denied_error(plugin, outputs, "tool_call");
    }

    // Check tool restriction: if `tools` is non-empty, only those are allowed.
    let allowed: Option<Vec<String>> = ctx.capabilities.iter().find_map(|c| {
        if let Capability::ToolCall { tools } = c {
            Some(tools.clone())
        } else {
            None
        }
    });

    if let Some(ref list) = allowed
        && !list.is_empty()
        && !list.contains(&req.tool)
    {
        return write_output(
            plugin,
            outputs,
            &super::HostError {
                error: format!("tool_call: tool '{}' is not in the allowed list", req.tool),
            },
        );
    }

    let executor = match &ctx.tool_executor {
        Some(e) => Arc::clone(e),
        None => return not_yet_connected_error(plugin, outputs, "host_call_tool"),
    };

    // Drop the lock before blocking on async to avoid starving other host calls.
    drop(ctx);

    let result = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?
        .block_on_async(executor.execute(&req.tool, req.input, 0))?
        .map_err(|e| extism::Error::msg(format!("tool execute failed: {e}")))?;

    // The executor returns a JSON string; parse it into a Value for the response.
    let output: serde_json::Value =
        serde_json::from_str(&result).unwrap_or(serde_json::Value::String(result));

    write_output(plugin, outputs, &CallToolResponse { output })
}
