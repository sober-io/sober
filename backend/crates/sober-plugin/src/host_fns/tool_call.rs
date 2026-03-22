//! Host function: cross-tool invocation.

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CallToolRequest, CapabilityKind, HostContext, capability_denied_error, not_yet_connected_error,
    read_input,
};

/// Calls another tool/plugin.
///
/// Requires the `ToolCall` capability.  Returns a stub error until the
/// backing service is wired in.
pub(crate) fn host_call_tool_impl(
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
