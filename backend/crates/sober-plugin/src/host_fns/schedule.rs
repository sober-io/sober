//! Host function: job scheduling.

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, HostContext, ScheduleRequest, capability_denied_error, not_yet_connected_error,
    read_input,
};

/// Creates or manages a scheduled job.
///
/// Requires the `Schedule` capability.  Returns a stub error until the
/// backing service is wired in.
pub(crate) fn host_schedule_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let _req: ScheduleRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::Schedule) {
        return capability_denied_error(plugin, outputs, "schedule");
    }

    not_yet_connected_error(plugin, outputs, "host_schedule")
}
