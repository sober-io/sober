//! Host function: secret reading.

use std::sync::Arc;

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, HostContext, ReadSecretRequest, ReadSecretResponse, capability_denied_error,
    not_yet_connected_error, read_input, write_output,
};

/// Reads a secret from the vault.
///
/// Requires the `SecretRead` capability.  Returns an error if no secret
/// backend is configured or if the plugin is running outside a user context.
pub(crate) fn host_read_secret_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: ReadSecretRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::SecretRead) {
        return capability_denied_error(plugin, outputs, "secret_read");
    }

    let backend = match &ctx.secret_backend {
        Some(b) => Arc::clone(b),
        None => return not_yet_connected_error(plugin, outputs, "host_read_secret"),
    };

    let user_id = match ctx.user_id {
        Some(id) => id,
        None => return Err(extism::Error::msg("no user context")),
    };

    // Drop the lock before blocking on async to avoid starving other host calls.
    drop(ctx);

    let value = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?
        .block_on_async(backend.read_secret(user_id, &req.name))?
        .map_err(|e| extism::Error::msg(format!("secret read failed: {e}")))?;

    write_output(plugin, outputs, &ReadSecretResponse { value })
}
