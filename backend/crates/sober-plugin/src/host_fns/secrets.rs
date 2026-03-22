//! Host function: secret reading.

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, HostContext, ReadSecretRequest, capability_denied_error,
    not_yet_connected_error, read_input,
};

/// Reads a secret from the vault.
///
/// Requires the `SecretRead` capability.  Returns a stub error until the
/// backing service is wired in.
pub(crate) fn host_read_secret_impl(
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
