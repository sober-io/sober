//! Host functions: plugin-scoped key-value storage.

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, HostContext, HostOk, KvDeleteRequest, KvGetRequest, KvGetResponse,
    KvListRequest, KvListResponse, KvSetRequest, capability_denied_error, read_input, write_output,
};

/// Reads a value from plugin-scoped key-value storage.
///
/// Requires the `KeyValue` capability.  Phase 1 uses an in-memory store.
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

    let kv = ctx
        .kv_store
        .lock()
        .map_err(|e| extism::Error::msg(format!("kv lock poisoned: {e}")))?;
    let value = kv.get(&req.key).cloned();

    let resp = KvGetResponse { value };
    write_output(plugin, outputs, &resp)
}

/// Writes a value to plugin-scoped key-value storage.
///
/// Requires the `KeyValue` capability.  Phase 1 uses an in-memory store.
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

    let mut kv = ctx
        .kv_store
        .lock()
        .map_err(|e| extism::Error::msg(format!("kv lock poisoned: {e}")))?;
    kv.insert(req.key, req.value);

    let ok = HostOk { ok: true };
    write_output(plugin, outputs, &ok)
}

/// Deletes a key from plugin-scoped key-value storage.
///
/// Requires the `KeyValue` capability.  Phase 1 uses an in-memory store.
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

    let mut kv = ctx
        .kv_store
        .lock()
        .map_err(|e| extism::Error::msg(format!("kv lock poisoned: {e}")))?;
    kv.remove(&req.key);

    let ok = HostOk { ok: true };
    write_output(plugin, outputs, &ok)
}

/// Lists keys in plugin-scoped key-value storage, optionally filtered by prefix.
///
/// Requires the `KeyValue` capability.  Phase 1 uses an in-memory store.
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

    let kv = ctx
        .kv_store
        .lock()
        .map_err(|e| extism::Error::msg(format!("kv lock poisoned: {e}")))?;

    let keys: Vec<String> = match &req.prefix {
        Some(prefix) => kv
            .keys()
            .filter(|k| k.starts_with(prefix.as_str()))
            .cloned()
            .collect(),
        None => kv.keys().cloned().collect(),
    };

    let resp = KvListResponse { keys };
    write_output(plugin, outputs, &resp)
}
