//! Host functions: plugin-scoped key-value storage.
//!
//! Operations are delegated to the [`KvBackend`] attached to the
//! [`HostContext`].  In production this is typically a [`PgKvBackend`];
//! in tests/offline mode it defaults to [`InMemoryKvBackend`].

use std::sync::Arc;

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, HostContext, HostOk, KvDeleteRequest, KvGetRequest, KvGetResponse,
    KvListRequest, KvListResponse, KvSetRequest, capability_denied_error, read_input, write_output,
};

/// Reads a value from plugin-scoped key-value storage.
///
/// Requires the `KeyValue` capability.
pub(crate) fn host_kv_get_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: KvGetRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::KeyValue) {
        return capability_denied_error(plugin, outputs, "key_value");
    }

    let backend = Arc::clone(&ctx.kv_backend);
    let plugin_id = ctx.plugin_id;
    let value = ctx
        .block_on_async(backend.get(plugin_id, &req.key))?
        .map_err(extism::Error::msg)?;

    let resp = KvGetResponse { value };
    write_output(plugin, outputs, &resp)
}

/// Writes a value to plugin-scoped key-value storage.
///
/// Requires the `KeyValue` capability.
pub(crate) fn host_kv_set_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: KvSetRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::KeyValue) {
        return capability_denied_error(plugin, outputs, "key_value");
    }

    let backend = Arc::clone(&ctx.kv_backend);
    let plugin_id = ctx.plugin_id;
    ctx.block_on_async(backend.set(plugin_id, &req.key, req.value))?
        .map_err(extism::Error::msg)?;

    let ok = HostOk { ok: true };
    write_output(plugin, outputs, &ok)
}

/// Deletes a key from plugin-scoped key-value storage.
///
/// Requires the `KeyValue` capability.
pub(crate) fn host_kv_delete_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: KvDeleteRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::KeyValue) {
        return capability_denied_error(plugin, outputs, "key_value");
    }

    let backend = Arc::clone(&ctx.kv_backend);
    let plugin_id = ctx.plugin_id;
    ctx.block_on_async(backend.delete(plugin_id, &req.key))?
        .map_err(extism::Error::msg)?;

    let ok = HostOk { ok: true };
    write_output(plugin, outputs, &ok)
}

/// Lists keys in plugin-scoped key-value storage, optionally filtered by prefix.
///
/// Requires the `KeyValue` capability.
pub(crate) fn host_kv_list_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: KvListRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::KeyValue) {
        return capability_denied_error(plugin, outputs, "key_value");
    }

    let backend = Arc::clone(&ctx.kv_backend);
    let plugin_id = ctx.plugin_id;
    let keys = ctx
        .block_on_async(backend.list_keys(plugin_id, req.prefix.as_deref()))?
        .map_err(extism::Error::msg)?;

    let resp = KvListResponse { keys };
    write_output(plugin, outputs, &resp)
}
