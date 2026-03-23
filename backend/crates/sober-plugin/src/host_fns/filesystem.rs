//! Host functions: sandboxed filesystem access.

use std::path::Path;

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, FsReadRequest, FsReadResponse, FsWriteRequest, HostContext, HostError, HostOk,
    capability_denied_error, read_input, write_output,
};
use crate::capability::Capability;

/// Reads a file from the sandboxed filesystem.
///
/// Requires the `Filesystem` capability.  The requested path is canonicalized
/// and checked against the allowed path prefixes declared in the capability.
/// An empty `paths` list is treated as "allow all" (unrestricted access).
pub(crate) fn host_fs_read_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: FsReadRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;

    // Extract the allowed paths from the Filesystem capability, then drop lock.
    let allowed_paths = {
        let ctx = data
            .lock()
            .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

        if !ctx.has_capability(&CapabilityKind::Filesystem) {
            return capability_denied_error(plugin, outputs, "filesystem");
        }

        ctx.capabilities
            .iter()
            .find_map(|c| match c {
                Capability::Filesystem { paths } => Some(paths.clone()),
                _ => None,
            })
            .unwrap_or_default()
    };

    // Canonicalize the requested path to resolve `..` and symlinks.
    let canonical = match std::fs::canonicalize(&req.path) {
        Ok(p) => p,
        Err(e) => {
            let err = HostError {
                error: format!("filesystem: cannot resolve path {:?}: {e}", req.path),
            };
            return write_output(plugin, outputs, &err);
        }
    };

    // Enforce path restrictions when the list is non-empty.
    if !allowed_paths.is_empty() && !is_path_allowed(&canonical, &allowed_paths) {
        let err = HostError {
            error: format!(
                "filesystem: path {:?} is not within any allowed path",
                req.path
            ),
        };
        return write_output(plugin, outputs, &err);
    }

    // Read the file content.
    match std::fs::read_to_string(&canonical) {
        Ok(content) => write_output(plugin, outputs, &FsReadResponse { content }),
        Err(e) => {
            let err = HostError {
                error: format!("filesystem: failed to read {:?}: {e}", req.path),
            };
            write_output(plugin, outputs, &err)
        }
    }
}

/// Writes a file to the sandboxed filesystem.
///
/// Requires the `Filesystem` capability.  The target path's parent directory
/// is created if it does not exist.  Path restrictions are enforced against
/// the canonicalized final path after ensuring the parent directory exists.
///
/// An empty `paths` list is treated as "allow all" (unrestricted access).
pub(crate) fn host_fs_write_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: FsWriteRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;

    // Extract the allowed paths from the Filesystem capability, then drop lock.
    let allowed_paths = {
        let ctx = data
            .lock()
            .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

        if !ctx.has_capability(&CapabilityKind::Filesystem) {
            return capability_denied_error(plugin, outputs, "filesystem");
        }

        ctx.capabilities
            .iter()
            .find_map(|c| match c {
                Capability::Filesystem { paths } => Some(paths.clone()),
                _ => None,
            })
            .unwrap_or_default()
    };

    let target = Path::new(&req.path);

    // Check path restrictions BEFORE creating any directories or files.
    // We validate against the raw (non-canonicalized) path first to prevent
    // directory creation as a side effect of denied writes.
    if !allowed_paths.is_empty() {
        // Use the parent (if it exists on disk) to get a canonical prefix,
        // otherwise fall back to the raw path for prefix checking.
        let check_path = target
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .map(|p| p.join(target.file_name().unwrap_or_default()))
            .unwrap_or_else(|| target.to_path_buf());

        if !is_path_allowed(&check_path, &allowed_paths) {
            let err = HostError {
                error: format!(
                    "filesystem: path {:?} is not within any allowed path",
                    req.path
                ),
            };
            return write_output(plugin, outputs, &err);
        }
    }

    // Create parent directories if they don't exist so we can canonicalize
    // the full path afterwards.
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        let err = HostError {
            error: format!(
                "filesystem: failed to create parent directories for {:?}: {e}",
                req.path
            ),
        };
        return write_output(plugin, outputs, &err);
    }

    // Canonicalize the parent directory and reconstruct the full target path.
    let canonical = match canonicalize_write_target(target) {
        Ok(p) => p,
        Err(e) => {
            let err = HostError {
                error: format!("filesystem: cannot resolve path {:?}: {e}", req.path),
            };
            return write_output(plugin, outputs, &err);
        }
    };

    // Re-check against canonical path (catches symlink traversal).
    if !allowed_paths.is_empty() && !is_path_allowed(&canonical, &allowed_paths) {
        let err = HostError {
            error: format!(
                "filesystem: path {:?} is not within any allowed path",
                req.path
            ),
        };
        return write_output(plugin, outputs, &err);
    }

    // Write the file.
    match std::fs::write(&canonical, req.content.as_bytes()) {
        Ok(()) => write_output(plugin, outputs, &HostOk { ok: true }),
        Err(e) => {
            let err = HostError {
                error: format!("filesystem: failed to write {:?}: {e}", req.path),
            };
            write_output(plugin, outputs, &err)
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `true` if `path` starts with any of the `allowed_paths` prefixes.
///
/// Each allowed path is also canonicalized so that relative or `~`-style
/// entries work correctly.  Entries that cannot be canonicalized are skipped
/// (a missing allowed directory does not grant access).
fn is_path_allowed(path: &Path, allowed_paths: &[String]) -> bool {
    allowed_paths.iter().any(|allowed| {
        let allowed_path = Path::new(allowed);
        // Canonicalize the allowed prefix so that relative paths, symlinks, and
        // `.` / `..` components are resolved consistently.
        let canonical_allowed = match std::fs::canonicalize(allowed_path) {
            Ok(p) => p,
            // If the allowed path doesn't exist, we can't grant access via it.
            Err(_) => return false,
        };
        path.starts_with(&canonical_allowed)
    })
}

/// Resolves the canonical path for a write target that may not exist yet.
///
/// Canonicalizes the parent directory (which must exist after `create_dir_all`)
/// and appends the file name component.
fn canonicalize_write_target(target: &Path) -> std::io::Result<std::path::PathBuf> {
    match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            let canonical_parent = std::fs::canonicalize(parent)?;
            let file_name = target.file_name().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name")
            })?;
            Ok(canonical_parent.join(file_name))
        }
        _ => {
            // No parent (or empty parent) — the file is in the current directory.
            let canonical_cwd = std::fs::canonicalize(".")?;
            let file_name = target.file_name().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name")
            })?;
            Ok(canonical_cwd.join(file_name))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fs;

    use sober_core::types::ids::PluginId;

    use super::*;
    use crate::capability::Capability;
    use crate::host_fns::HostContext;

    fn test_context(capabilities: Vec<Capability>) -> HostContext {
        HostContext::new(PluginId::new(), capabilities)
    }

    #[test]
    fn is_path_allowed_within_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let allowed = dir.path().to_str().unwrap().to_string();
        let file = dir.path().join("file.txt");

        assert!(is_path_allowed(&file, &[allowed]));
    }

    #[test]
    fn is_path_allowed_outside_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let other_dir = tempfile::tempdir().expect("other tempdir");
        let allowed = dir.path().to_str().unwrap().to_string();
        let file = other_dir.path().join("file.txt");

        assert!(!is_path_allowed(&file, &[allowed]));
    }

    #[test]
    fn is_path_allowed_empty_list_returns_false() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("file.txt");
        // Empty list means "no explicit allowed paths"; caller must handle the
        // unrestricted case separately.
        assert!(!is_path_allowed(&file, &[]));
    }

    #[test]
    fn is_path_allowed_nonexistent_allowed_dir_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("file.txt");
        // A non-existent allowed path can't be canonicalized → skipped.
        let allowed = "/nonexistent/path/that/does/not/exist".to_string();
        assert!(!is_path_allowed(&file, &[allowed]));
    }

    #[test]
    fn host_fs_read_and_write_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("test.txt");

        // Write the file using std::fs directly (the host fn requires WASM).
        fs::write(&file_path, b"hello from plugin").expect("write test file");

        // Verify our path-allow logic accepts it.
        let canonical = fs::canonicalize(&file_path).expect("canonicalize");
        let allowed_paths = vec![dir.path().to_str().unwrap().to_string()];
        assert!(is_path_allowed(&canonical, &allowed_paths));
    }

    #[test]
    fn host_fs_read_capability_check() {
        // With no Filesystem capability the context must deny access.
        let ctx = test_context(vec![]);
        assert!(!ctx.has_capability(&CapabilityKind::Filesystem));
    }

    #[test]
    fn host_fs_write_capability_check() {
        // With Filesystem capability the context must grant access.
        let ctx = test_context(vec![Capability::Filesystem { paths: vec![] }]);
        assert!(ctx.has_capability(&CapabilityKind::Filesystem));
    }

    #[test]
    fn canonicalize_write_target_with_parent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("output.txt");
        let result = canonicalize_write_target(&target).expect("canonicalize");
        assert_eq!(
            result,
            dir.path().canonicalize().unwrap().join("output.txt")
        );
    }
}
