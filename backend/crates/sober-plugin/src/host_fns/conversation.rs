//! Host function: conversation history reading.

use std::sync::Arc;

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, ConversationReadRequest, ConversationReadResponse, HostContext,
    capability_denied_error, not_yet_connected_error, read_input, write_output,
};

/// Reads conversation history.
///
/// Requires the `ConversationRead` capability.  Returns up to `limit` recent
/// messages from the specified conversation.
pub(crate) fn host_conversation_read_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: ConversationReadRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::ConversationRead) {
        return capability_denied_error(plugin, outputs, "conversation_read");
    }

    let backend = match &ctx.conversation_backend {
        Some(b) => Arc::clone(b),
        None => return not_yet_connected_error(plugin, outputs, "host_conversation_read"),
    };

    let messages = ctx
        .block_on_async(backend.list_messages(&req.conversation_id, req.limit))?
        .map_err(extism::Error::msg)?;

    write_output(plugin, outputs, &ConversationReadResponse { messages })
}
