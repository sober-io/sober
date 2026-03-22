//! Host functions: plugin-scoped key-value storage.
//!
//! When a database pool and runtime handle are available, operations are
//! persisted to the `plugin_kv_data` table (JSONB per plugin).  Otherwise
//! the in-memory `HashMap` fallback is used (tests, offline mode).

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, HostContext, HostOk, KvDeleteRequest, KvGetRequest, KvGetResponse,
    KvListRequest, KvListResponse, KvSetRequest, capability_denied_error, read_input, write_output,
};

/// Reads a value from plugin-scoped key-value storage.
///
/// Requires the `KeyValue` capability.  Tries DB first, falls back to in-memory.
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

    // Try DB first — clone what we need, drop lock, then block_on
    if ctx.db_pool.is_some() && ctx.runtime_handle.is_some() {
        let plugin_id = ctx.plugin_id;
        let pool = ctx.db_pool.clone().expect("checked above");
        let handle = ctx.runtime_handle.clone().expect("checked above");
        let key = req.key.clone();
        drop(ctx);

        let result = handle.block_on(async {
            sqlx::query_as::<_, (serde_json::Value,)>(
                "SELECT data->$2 FROM plugin_kv_data WHERE plugin_id = $1",
            )
            .bind(plugin_id.as_uuid())
            .bind(&key)
            .fetch_optional(&pool)
            .await
        });
        let value = result
            .map_err(|e| extism::Error::msg(format!("kv get failed: {e}")))?
            .and_then(|(v,)| if v.is_null() { None } else { Some(v) });
        let resp = KvGetResponse { value };
        return write_output(plugin, outputs, &resp);
    }

    // Fallback to in-memory
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
/// Requires the `KeyValue` capability.  Tries DB first, falls back to in-memory.
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

    // Try DB first — clone what we need, drop lock, then block_on
    if ctx.db_pool.is_some() && ctx.runtime_handle.is_some() {
        let plugin_id = ctx.plugin_id;
        let pool = ctx.db_pool.clone().expect("checked above");
        let handle = ctx.runtime_handle.clone().expect("checked above");
        let key = req.key.clone();
        let value = req.value.clone();
        drop(ctx);

        handle
            .block_on(async {
                sqlx::query(
                    "INSERT INTO plugin_kv_data (plugin_id, data, updated_at) \
                     VALUES ($1, jsonb_build_object($2::text, $3), now()) \
                     ON CONFLICT (plugin_id) DO UPDATE \
                     SET data = plugin_kv_data.data || jsonb_build_object($2::text, $3), \
                         updated_at = now()",
                )
                .bind(plugin_id.as_uuid())
                .bind(&key)
                .bind(&value)
                .execute(&pool)
                .await
            })
            .map_err(|e| extism::Error::msg(format!("kv set failed: {e}")))?;

        let ok = HostOk { ok: true };
        return write_output(plugin, outputs, &ok);
    }

    // Fallback to in-memory
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
/// Requires the `KeyValue` capability.  Tries DB first, falls back to in-memory.
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

    // Try DB first — clone what we need, drop lock, then block_on
    if ctx.db_pool.is_some() && ctx.runtime_handle.is_some() {
        let plugin_id = ctx.plugin_id;
        let pool = ctx.db_pool.clone().expect("checked above");
        let handle = ctx.runtime_handle.clone().expect("checked above");
        let key = req.key.clone();
        drop(ctx);

        handle
            .block_on(async {
                sqlx::query(
                    "UPDATE plugin_kv_data SET data = data - $2, updated_at = now() \
                     WHERE plugin_id = $1",
                )
                .bind(plugin_id.as_uuid())
                .bind(&key)
                .execute(&pool)
                .await
            })
            .map_err(|e| extism::Error::msg(format!("kv delete failed: {e}")))?;

        let ok = HostOk { ok: true };
        return write_output(plugin, outputs, &ok);
    }

    // Fallback to in-memory
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
/// Requires the `KeyValue` capability.  Tries DB first, falls back to in-memory.
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

    // Try DB first — clone what we need, drop lock, then block_on
    if ctx.db_pool.is_some() && ctx.runtime_handle.is_some() {
        let plugin_id = ctx.plugin_id;
        let pool = ctx.db_pool.clone().expect("checked above");
        let handle = ctx.runtime_handle.clone().expect("checked above");
        let prefix = req.prefix.clone();
        drop(ctx);

        let rows: Vec<(String,)> = handle
            .block_on(async {
                sqlx::query_as(
                    "SELECT k FROM plugin_kv_data, jsonb_object_keys(data) AS k \
                     WHERE plugin_id = $1",
                )
                .bind(plugin_id.as_uuid())
                .fetch_all(&pool)
                .await
            })
            .map_err(|e| extism::Error::msg(format!("kv list failed: {e}")))?;

        let mut keys: Vec<String> = rows.into_iter().map(|(k,)| k).collect();
        if let Some(prefix) = &prefix {
            keys.retain(|k| k.starts_with(prefix.as_str()));
        }

        let resp = KvListResponse { keys };
        return write_output(plugin, outputs, &resp);
    }

    // Fallback to in-memory
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
