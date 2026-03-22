//! Host functions: sandboxed filesystem access.

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, FsReadRequest, FsWriteRequest, HostContext, capability_denied_error,
    not_yet_connected_error, read_input,
};

/// Reads a file from the sandboxed filesystem.
///
/// Requires the `Filesystem` capability.  Returns a stub error until the
/// backing service is wired in.
pub(crate) fn host_fs_read_impl(
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
/// Requires the `Filesystem` capability.  Returns a stub error until the
/// backing service is wired in.
pub(crate) fn host_fs_write_impl(
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
