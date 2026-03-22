//! Host functions: memory/context system access.

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, HostContext, MemoryQueryRequest, MemoryWriteRequest, capability_denied_error,
    not_yet_connected_error, read_input,
};

/// Queries the memory/context system.
///
/// Requires the `MemoryRead` capability.  Returns a stub error until the
/// backing service is wired in.
pub(crate) fn host_memory_query_impl(
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
/// Requires the `MemoryWrite` capability.  Returns a stub error until the
/// backing service is wired in.
pub(crate) fn host_memory_write_impl(
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
