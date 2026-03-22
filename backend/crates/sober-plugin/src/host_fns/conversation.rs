//! Host function: conversation history reading.

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, ConversationReadRequest, HostContext, capability_denied_error,
    not_yet_connected_error, read_input,
};

/// Reads conversation history.
///
/// Requires the `ConversationRead` capability.  Returns a stub error until the
/// backing service is wired in.
pub(crate) fn host_conversation_read_impl(
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
