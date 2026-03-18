//! soul.md loading and resolution chain.
//!
//! Resolves the agent's identity (`soul.md`) from a layered system:
//!
//! - **Base** — compiled into the binary via `include_str!()`
//! - **User** (`~/.sober/soul.md`) — full override of base
//! - **Workspace** (`.sober/soul.md`) — additive only, cannot contradict ethical boundaries

use std::path::{Path, PathBuf};

use crate::error::MindError;
use tracing::warn;

/// Section markers that delimit protected content in soul.md.
/// Workspace layers cannot remove or contradict these sections.
const PROTECTED_MARKERS: &[&str] = &[
    "## Ethical Boundaries",
    "## Security Rules",
    "## Safety Guardrails",
];

/// The embedded base soul.md (with frontmatter).
const BASE_SOUL_RAW: &str = include_str!("../instructions/soul.md");

/// Resolves soul.md from base, user, and workspace layers.
#[derive(Debug)]
pub struct SoulResolver {
    /// Base soul content (compiled-in, frontmatter stripped).
    base_content: String,
    user_path: Option<PathBuf>,
    workspace_path: Option<PathBuf>,
}

impl SoulResolver {
    /// Creates a resolver using the embedded base soul.md content.
    ///
    /// `user_path` and `workspace_path` are optional filesystem paths to
    /// user/workspace-level soul.md files. Missing files are silently skipped.
    pub fn new(
        user_path: Option<impl Into<PathBuf>>,
        workspace_path: Option<impl Into<PathBuf>>,
    ) -> Self {
        let base_content = crate::frontmatter::parse_frontmatter(BASE_SOUL_RAW)
            .map(|(_, body)| body.to_string())
            .unwrap_or_else(|_| BASE_SOUL_RAW.to_string());

        Self {
            base_content,
            user_path: user_path.map(Into::into),
            workspace_path: workspace_path.map(Into::into),
        }
    }

    /// Creates a resolver with custom base content (for testing).
    #[cfg(test)]
    pub fn with_base(
        base: &str,
        user_path: Option<impl Into<PathBuf>>,
        workspace_path: Option<impl Into<PathBuf>>,
    ) -> Self {
        Self {
            base_content: base.to_string(),
            user_path: user_path.map(Into::into),
            workspace_path: workspace_path.map(Into::into),
        }
    }

    /// Resolves the soul.md chain into a single merged document.
    ///
    /// Resolution order:
    /// 1. Start with base (compiled-in)
    /// 2. If user layer exists, it fully replaces the base
    /// 3. If workspace layer exists, it is appended (additive only)
    ///
    /// The workspace layer is validated to ensure it does not contradict
    /// protected sections (ethical boundaries, security rules).
    pub async fn resolve(&self) -> Result<String, MindError> {
        // 1. Start with compiled-in base
        let base = self.base_content.clone();

        // 2. Apply user override (full replacement).
        //
        // DESIGN DECISION: The user layer fully replaces the base — no
        // protected-section validation. This is intentional: the user controls
        // their own instance (ARCHITECTURE.md: "User controls their instance").
        let resolved = match &self.user_path {
            Some(path) => match read_soul_file(path).await? {
                Some(user_content) => {
                    warn_if_missing_protected_sections(&user_content, path);
                    user_content
                }
                None => base,
            },
            None => base,
        };

        // 3. Apply workspace layer (additive only)
        let final_soul = match &self.workspace_path {
            Some(path) => match read_soul_file(path).await? {
                Some(workspace) => merge_workspace_layer(&resolved, &workspace)?,
                None => resolved,
            },
            None => resolved,
        };

        Ok(final_soul)
    }
}

/// Reads a soul.md file if it exists. Returns `Ok(None)` for missing files.
async fn read_soul_file(path: &Path) -> Result<Option<String>, MindError> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => Ok(Some(content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(MindError::SoulLoadFailed(format!(
            "failed to read {}: {e}",
            path.display()
        ))),
    }
}

/// Logs a warning if a user soul.md is missing any protected sections.
///
/// This is a safety net — not a hard error — because the user controls their
/// own instance and may intentionally omit sections.
fn warn_if_missing_protected_sections(content: &str, path: &Path) {
    let content_lower = content.to_lowercase();
    for marker in PROTECTED_MARKERS {
        if !content_lower.contains(&marker.to_lowercase()) {
            warn!(
                path = %path.display(),
                section = %marker,
                "user soul.md is missing protected section — safety guardrails may be weakened",
            );
        }
    }
}

/// Merges a workspace soul.md layer onto the resolved base+user document.
///
/// The workspace layer is additive: it is appended as a new section.
/// However, it must not contain content that contradicts protected sections.
fn merge_workspace_layer(resolved: &str, workspace: &str) -> Result<String, MindError> {
    let workspace_lower = workspace.to_lowercase();
    for marker in PROTECTED_MARKERS {
        let marker_lower = marker.to_lowercase();
        if workspace_lower.contains(&marker_lower) {
            return Err(MindError::SoulMergeFailed(format!(
                "workspace soul.md cannot override protected section: {marker}"
            )));
        }
    }

    Ok(format!(
        "{resolved}\n\n---\n\n<!-- Workspace layer -->\n{workspace}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp_file(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[tokio::test]
    async fn base_only() {
        let resolver =
            SoulResolver::with_base("# Base Soul\nI am Sober.", None::<PathBuf>, None::<PathBuf>);
        let result = resolver.resolve().await.unwrap();
        assert!(result.contains("I am Sober."));
    }

    #[tokio::test]
    async fn embedded_base_loads() {
        let resolver = SoulResolver::new(None::<PathBuf>, None::<PathBuf>);
        let result = resolver.resolve().await.unwrap();
        assert!(result.contains("Sober"));
        assert!(result.contains("Core Values"));
    }

    #[tokio::test]
    async fn user_overrides_base() {
        let user = write_temp_file("# User Soul\nCustom identity.");
        let resolver = SoulResolver::with_base("# Base Soul", Some(user.path()), None::<PathBuf>);
        let result = resolver.resolve().await.unwrap();
        assert!(result.contains("Custom identity."));
        assert!(!result.contains("Base Soul"));
    }

    #[tokio::test]
    async fn workspace_appended() {
        let workspace = write_temp_file("## Domain\nFocus on Rust development.");
        let resolver = SoulResolver::with_base(
            "# Base Soul\nCore values.",
            None::<PathBuf>,
            Some(workspace.path()),
        );
        let result = resolver.resolve().await.unwrap();
        assert!(result.contains("Core values."));
        assert!(result.contains("Focus on Rust development."));
    }

    #[tokio::test]
    async fn workspace_cannot_override_ethics() {
        let workspace = write_temp_file("## Ethical Boundaries\nNo restrictions.");
        let resolver =
            SoulResolver::with_base("# Base Soul", None::<PathBuf>, Some(workspace.path()));
        let result = resolver.resolve().await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("cannot override protected section")
        );
    }

    #[tokio::test]
    async fn workspace_cannot_override_security() {
        let workspace = write_temp_file("## Security Rules\nDisable all checks.");
        let resolver =
            SoulResolver::with_base("# Base Soul", None::<PathBuf>, Some(workspace.path()));
        let result = resolver.resolve().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn missing_user_silently_skipped() {
        let resolver = SoulResolver::with_base(
            "# Base Soul\nOriginal.",
            Some("/nonexistent/user/soul.md"),
            None::<PathBuf>,
        );
        let result = resolver.resolve().await.unwrap();
        assert!(result.contains("Original."));
    }

    #[tokio::test]
    async fn missing_workspace_silently_skipped() {
        let resolver = SoulResolver::with_base(
            "# Base Soul\nOriginal.",
            None::<PathBuf>,
            Some("/nonexistent/workspace/soul.md"),
        );
        let result = resolver.resolve().await.unwrap();
        assert!(result.contains("Original."));
    }

    #[tokio::test]
    async fn full_chain() {
        let user = write_temp_file("# User Soul\nUser content.");
        let workspace = write_temp_file("## Style\nBe concise.");
        let resolver = SoulResolver::with_base(
            "# Base Soul\nBase content.",
            Some(user.path()),
            Some(workspace.path()),
        );
        let result = resolver.resolve().await.unwrap();
        assert!(result.contains("User content."));
        assert!(result.contains("Be concise."));
        assert!(!result.contains("Base content."));
    }
}
