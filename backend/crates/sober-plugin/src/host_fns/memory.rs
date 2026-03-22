//! Host functions: memory/context system access.

use std::sync::Arc;

use extism::{CurrentPlugin, UserData, Val};

use crate::capability::Capability;

use super::{
    CapabilityKind, HostContext, HostOk, MemoryQueryRequest, MemoryQueryResponse,
    MemoryWriteRequest, capability_denied_error, not_yet_connected_error, read_input, write_output,
};

/// Queries the memory/context system.
///
/// Requires the `MemoryRead` capability.  If the capability carries a non-empty
/// scope list, the requested scope (if any) must appear in that list.
pub(crate) fn host_memory_query_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: MemoryQueryRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::MemoryRead) {
        return capability_denied_error(plugin, outputs, "memory_read");
    }

    // Extract allowed scopes from the MemoryRead capability.
    let allowed_scopes: Vec<String> = ctx
        .capabilities
        .iter()
        .find_map(|c| {
            if let Capability::MemoryRead { scopes } = c {
                Some(scopes.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();

    // Validate the requested scope against the allowed list (if restricted).
    if !allowed_scopes.is_empty()
        && let Some(ref requested_scope) = req.scope
        && !allowed_scopes.iter().any(|s| s == requested_scope)
    {
        return capability_denied_error(plugin, outputs, "memory_read: scope not allowed");
    }

    let backend = match &ctx.memory_backend {
        Some(b) => Arc::clone(b),
        None => return not_yet_connected_error(plugin, outputs, "host_memory_query"),
    };

    let user_id = match ctx.user_id {
        Some(id) => id,
        None => {
            let err = super::HostError {
                error: "host_memory_query: no user context".to_string(),
            };
            return write_output(plugin, outputs, &err);
        }
    };

    let results = ctx
        .block_on_async(backend.search(user_id, &req.query, req.scope.as_deref(), req.limit))?
        .map_err(extism::Error::msg)?;

    write_output(plugin, outputs, &MemoryQueryResponse { results })
}

/// Writes to the memory/context system.
///
/// Requires the `MemoryWrite` capability.  If the capability carries a non-empty
/// scope list, the requested scope (if any) must appear in that list.
pub(crate) fn host_memory_write_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: MemoryWriteRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::MemoryWrite) {
        return capability_denied_error(plugin, outputs, "memory_write");
    }

    // Extract allowed scopes from the MemoryWrite capability.
    let allowed_scopes: Vec<String> = ctx
        .capabilities
        .iter()
        .find_map(|c| {
            if let Capability::MemoryWrite { scopes } = c {
                Some(scopes.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();

    // Validate the requested scope against the allowed list (if restricted).
    if !allowed_scopes.is_empty()
        && let Some(ref requested_scope) = req.scope
        && !allowed_scopes.iter().any(|s| s == requested_scope)
    {
        return capability_denied_error(plugin, outputs, "memory_write: scope not allowed");
    }

    let backend = match &ctx.memory_backend {
        Some(b) => Arc::clone(b),
        None => return not_yet_connected_error(plugin, outputs, "host_memory_write"),
    };

    let user_id = match ctx.user_id {
        Some(id) => id,
        None => {
            let err = super::HostError {
                error: "host_memory_write: no user context".to_string(),
            };
            return write_output(plugin, outputs, &err);
        }
    };

    ctx.block_on_async(backend.store(
        user_id,
        &req.content,
        req.scope.as_deref(),
        req.metadata.clone(),
    ))?
    .map_err(extism::Error::msg)?;

    write_output(plugin, outputs, &HostOk { ok: true })
}
