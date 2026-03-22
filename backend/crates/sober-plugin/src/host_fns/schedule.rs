//! Host function: job scheduling.

use std::sync::Arc;

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, HostContext, ScheduleRequest, ScheduleResponse, capability_denied_error,
    not_yet_connected_error, read_input, write_output,
};

/// Creates or manages a scheduled job.
///
/// Requires the `Schedule` capability.  Calls the [`ScheduleBackend`] to
/// persist the job and returns the assigned job ID.
pub(crate) fn host_schedule_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: ScheduleRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;
    let ctx = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

    if !ctx.has_capability(&CapabilityKind::Schedule) {
        return capability_denied_error(plugin, outputs, "schedule");
    }

    let backend = match &ctx.schedule_backend {
        Some(b) => Arc::clone(b),
        None => return not_yet_connected_error(plugin, outputs, "host_schedule"),
    };

    let plugin_id = ctx.plugin_id;

    // Drop the lock before blocking on async to avoid starving other host calls.
    drop(ctx);

    let job_id = data
        .lock()
        .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?
        .block_on_async(backend.create_job(plugin_id, &req.schedule, req.payload))?
        .map_err(|e| extism::Error::msg(format!("schedule create_job failed: {e}")))?;

    write_output(plugin, outputs, &ScheduleResponse { job_id })
}
