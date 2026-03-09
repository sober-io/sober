//! Runtime detection of sandbox dependency binaries.
//!
//! Checks that `bwrap` (bubblewrap) and optionally `socat` are available
//! on the system PATH. Results are cached after the first lookup.

use std::path::PathBuf;
use std::sync::OnceLock;

use tracing::info;

use crate::error::SandboxError;

static BWRAP_PATH: OnceLock<PathBuf> = OnceLock::new();
static SOCAT_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Detect the `bwrap` binary on PATH.
///
/// The result is cached after the first successful call.
///
/// # Errors
///
/// Returns [`SandboxError::BwrapNotFound`] if the binary is not on PATH.
pub fn detect_bwrap() -> Result<PathBuf, SandboxError> {
    if let Some(path) = BWRAP_PATH.get() {
        return Ok(path.clone());
    }

    let path = which::which("bwrap").map_err(|_| SandboxError::BwrapNotFound)?;
    info!(path = %path.display(), "detected bwrap binary");

    // Race-safe: if another thread set it first, use theirs.
    Ok(BWRAP_PATH.get_or_init(|| path).clone())
}

/// Detect the `socat` binary on PATH.
///
/// The result is cached after the first successful call. Only required
/// when using [`NetMode::AllowedDomains`](crate::policy::NetMode::AllowedDomains).
///
/// # Errors
///
/// Returns [`SandboxError::SocatNotFound`] if the binary is not on PATH.
pub fn detect_socat() -> Result<PathBuf, SandboxError> {
    if let Some(path) = SOCAT_PATH.get() {
        return Ok(path.clone());
    }

    let path = which::which("socat").map_err(|_| SandboxError::SocatNotFound)?;
    info!(path = %path.display(), "detected socat binary");

    Ok(SOCAT_PATH.get_or_init(|| path).clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_bwrap_returns_result() {
        // This test is environment-dependent. On systems without bwrap,
        // it will return BwrapNotFound — both outcomes are valid.
        let result = detect_bwrap();
        match result {
            Ok(path) => assert!(path.exists()),
            Err(SandboxError::BwrapNotFound) => {} // expected on some systems
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn detect_socat_returns_result() {
        let result = detect_socat();
        match result {
            Ok(path) => assert!(path.exists()),
            Err(SandboxError::SocatNotFound) => {}
            Err(e) => panic!("unexpected error: {e}"),
        }
    }
}
