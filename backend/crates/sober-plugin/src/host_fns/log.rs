//! Host function: structured logging.

use extism::{CurrentPlugin, UserData, Val};
use tracing::{debug, error, info, trace, warn};

use super::{HostContext, HostOk, LogRequest, read_input, write_output};

/// Structured logging from the plugin.
///
/// Always available — no capability gate required.  Maps plugin log levels
/// to `tracing` levels on the host side.
pub(crate) fn host_log_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: LogRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;
    let plugin_id = ctx.plugin_id;

    match req.level.to_lowercase().as_str() {
        "trace" => trace!(plugin_id = %plugin_id, fields = ?req.fields, "{}", req.message),
        "debug" => debug!(plugin_id = %plugin_id, fields = ?req.fields, "{}", req.message),
        "info" => info!(plugin_id = %plugin_id, fields = ?req.fields, "{}", req.message),
        "warn" => warn!(plugin_id = %plugin_id, fields = ?req.fields, "{}", req.message),
        "error" => error!(plugin_id = %plugin_id, fields = ?req.fields, "{}", req.message),
        _ => info!(plugin_id = %plugin_id, fields = ?req.fields, "[{}] {}", req.level, req.message),
    }

    let ok = HostOk { ok: true };
    write_output(plugin, outputs, &ok)
}
