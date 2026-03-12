//! Process-level execution sandboxing via bubblewrap (bwrap).
//!
//! `sober-sandbox` provides configurable filesystem, network, PID, and IPC
//! isolation for all process execution in the Sõber system. Sandbox policies
//! are resolved from a layered config chain (tool -> workspace -> user -> default).

pub mod audit;
pub mod bwrap;
pub mod command_policy;
pub mod config;
pub mod detect;
pub mod error;
pub mod policy;
pub mod proxy;
pub mod resolve;
pub mod risk;

pub use audit::{ExecutionOutcome, ExecutionTrigger, SandboxAuditEntry};
pub use bwrap::{BwrapSandbox, SandboxResult};
pub use command_policy::CommandPolicy;
pub use config::SandboxConfig;
pub use error::SandboxError;
pub use policy::{NetMode, SandboxPolicy, SandboxProfile};
pub use resolve::resolve_policy;
pub use risk::RiskLevel;

/// Check that required runtime dependencies (bwrap, optionally socat) are
/// available on the system PATH.
///
/// Intended to be called at application startup to fail fast if dependencies
/// are missing.
///
/// # Errors
///
/// Returns [`SandboxError::BwrapNotFound`] if bwrap is not on PATH.
pub fn check_runtime_deps() -> Result<(), SandboxError> {
    detect::detect_bwrap()?;
    // socat is only required when AllowedDomains mode is used,
    // so we don't fail startup if it's missing.
    if let Err(SandboxError::SocatNotFound) = detect::detect_socat() {
        tracing::warn!("socat not found — AllowedDomains network mode will not work");
    }
    Ok(())
}
