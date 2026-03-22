//! Host function: metric emission.

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, EmitMetricRequest, HostContext, HostError, HostOk, capability_denied_error,
    read_input, write_output,
};

/// Emits a metric (counter, gauge, or histogram sample).
///
/// Requires the `Metrics` capability.  All metric names are prefixed with
/// `sober_plugin_` to namespace them away from core metrics.  Supported
/// kinds: `"counter"`, `"gauge"`, `"histogram"`.
pub(crate) fn host_emit_metric_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: EmitMetricRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;

    // Check capability then drop the lock before doing any work.
    {
        let ctx = data
            .lock()
            .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

        if !ctx.has_capability(&CapabilityKind::Metrics) {
            return capability_denied_error(plugin, outputs, "metrics");
        }
    }

    let name = format!("sober_plugin_{}", req.name);

    // Convert labels into owned key-value pairs for the metrics macros.
    let labels: Vec<(String, String)> = req.labels.into_iter().collect();

    match req.kind.as_str() {
        "counter" => {
            metrics::counter!(name.clone(), &labels).increment(req.value as u64);
        }
        "gauge" => {
            metrics::gauge!(name.clone(), &labels).set(req.value);
        }
        "histogram" => {
            metrics::histogram!(name.clone(), &labels).record(req.value);
        }
        other => {
            let err = HostError {
                error: format!(
                    "unsupported metric kind: {other:?} (expected counter, gauge, or histogram)"
                ),
            };
            return write_output(plugin, outputs, &err);
        }
    }

    let ok = HostOk { ok: true };
    write_output(plugin, outputs, &ok)
}
