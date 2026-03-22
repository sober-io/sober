//! Job scheduling backend trait.
//!
//! [`ScheduleBackend`] provides an object-safe interface for creating
//! scheduled jobs from within a plugin.  Implementations will typically
//! delegate to `sober-scheduler` via gRPC.

use std::future::Future;
use std::pin::Pin;

use sober_core::types::ids::PluginId;

/// Object-safe backend for creating scheduled jobs.
///
/// Plugins can schedule recurring or one-shot jobs that will fire at the
/// specified time(s).  The scheduler persists the job and delivers the
/// payload when it triggers.
pub trait ScheduleBackend: Send + Sync {
    /// Creates a scheduled job. Returns the job ID.
    fn create_job(
        &self,
        plugin_id: PluginId,
        schedule: &str,
        payload: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;
}

// ---------------------------------------------------------------------------
// Compile-time assertions
// ---------------------------------------------------------------------------

// ScheduleBackend is object-safe and dyn-compatible.
#[allow(dead_code)]
const _: () = {
    fn assert_object_safe(_: &dyn ScheduleBackend) {}
};

// Arc<dyn ScheduleBackend> is Send + Sync.
#[allow(dead_code)]
const _: () = {
    fn assert_send_sync<T: Send + Sync>() {}
    fn check() {
        assert_send_sync::<std::sync::Arc<dyn ScheduleBackend>>();
    }
};
