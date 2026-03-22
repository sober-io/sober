//! Host function: LLM completion.

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, HostContext, LlmCompleteRequest, capability_denied_error,
    not_yet_connected_error, read_input,
};

/// Sends a prompt to an LLM provider.
///
/// Requires the `LlmCall` capability.  Returns a stub error until the
/// backing service is wired in.
pub(crate) fn host_llm_complete_impl(
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
