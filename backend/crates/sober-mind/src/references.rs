//! `@path` reference resolution for instruction files.
//!
//! Instruction files may contain `@path` references that embed external file
//! content. `.md` files are inlined raw; other extensions are wrapped in
//! fenced code blocks.

use std::path::{Path, PathBuf};

use regex::Regex;

use crate::error::MindError;

/// Resolves `@path` references in instruction file content.
#[derive(Debug)]
pub struct ReferenceResolver {
    root_dir: PathBuf,
}

impl ReferenceResolver {
    /// Creates a resolver that resolves paths relative to `root_dir`.
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }

    /// Resolves all `@path` references in the content (single-level, no recursion).
    ///
    /// - `.md` files are inlined raw
    /// - Other extensions are wrapped in fenced code blocks with the extension as language
    /// - Missing files produce an error
    ///
    /// References must be preceded by whitespace or start of line to avoid
    /// matching email addresses (e.g., `user@example.com`).
    pub fn resolve(&self, content: &str) -> Result<String, MindError> {
        // Match @path references. The regex itself matches all @path patterns;
        // we filter out email-like matches (preceded by a word character) in
        // the loop below.
        let re = Regex::new(r"@([a-zA-Z0-9_./-]+\.[a-zA-Z0-9]+)").expect("static regex is valid");

        let mut result = String::with_capacity(content.len());
        let mut last_end = 0;

        for cap in re.captures_iter(content) {
            let full_match = cap.get(0).expect("match exists");
            let start = full_match.start();

            // Skip email-like matches: if the character before `@` is
            // alphanumeric, this is likely an email address, not a reference.
            if start > 0 {
                let prev_byte = content.as_bytes()[start - 1];
                if prev_byte.is_ascii_alphanumeric() || prev_byte == b'_' {
                    continue;
                }
            }

            let ref_path = &cap[1];
            result.push_str(&content[last_end..start]);

            let resolved = self.resolve_single(ref_path)?;
            result.push_str(&resolved);

            last_end = full_match.end();
        }

        result.push_str(&content[last_end..]);
        Ok(result)
    }

    /// Resolves a single `@path` reference with path traversal protection.
    fn resolve_single(&self, ref_path: &str) -> Result<String, MindError> {
        let full_path = self.root_dir.join(ref_path);

        // Canonicalize and verify the resolved path stays within root_dir
        let canonical = full_path.canonicalize().map_err(|e| {
            MindError::ReferenceResolutionFailed(format!(
                "failed to resolve @{ref_path} ({}): {e}",
                full_path.display()
            ))
        })?;

        let canonical_root = self.root_dir.canonicalize().map_err(|e| {
            MindError::ReferenceResolutionFailed(format!(
                "failed to canonicalize root dir {}: {e}",
                self.root_dir.display()
            ))
        })?;

        if !canonical.starts_with(&canonical_root) {
            return Err(MindError::ReferenceResolutionFailed(format!(
                "@{ref_path} resolves outside root directory (path traversal blocked)"
            )));
        }

        let content = std::fs::read_to_string(&canonical).map_err(|e| {
            MindError::ReferenceResolutionFailed(format!(
                "failed to read @{ref_path} ({}): {e}",
                full_path.display()
            ))
        })?;

        let extension = Path::new(ref_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        if extension == "md" {
            Ok(content)
        } else {
            Ok(format!("```{extension}\n{content}\n```"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn resolves_md_inline() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("extra.md"), "# Extra\nMore content.").unwrap();

        let resolver = ReferenceResolver::new(dir.path());
        let result = resolver.resolve("Before @extra.md after").unwrap();
        assert_eq!(result, "Before # Extra\nMore content. after");
    }

    #[test]
    fn resolves_non_md_as_code_block() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("config.toml"), "[section]\nkey = \"val\"").unwrap();

        let resolver = ReferenceResolver::new(dir.path());
        let result = resolver.resolve("See @config.toml here").unwrap();
        assert!(result.contains("```toml\n[section]\nkey = \"val\"\n```"));
    }

    #[test]
    fn missing_file_errors() {
        let dir = TempDir::new().unwrap();
        let resolver = ReferenceResolver::new(dir.path());
        let result = resolver.resolve("Ref @missing.md here");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing.md"));
    }

    #[test]
    fn does_not_match_email_addresses() {
        let dir = TempDir::new().unwrap();
        let resolver = ReferenceResolver::new(dir.path());
        let result = resolver
            .resolve("Contact user@example.com for help")
            .unwrap();
        assert_eq!(result, "Contact user@example.com for help");
    }

    #[test]
    fn matches_at_start_of_line() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("file.md"), "content").unwrap();

        let resolver = ReferenceResolver::new(dir.path());
        let result = resolver.resolve("@file.md here").unwrap();
        assert_eq!(result, "content here");
    }

    #[test]
    fn multiple_references() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.md"), "AAA").unwrap();
        fs::write(dir.path().join("b.md"), "BBB").unwrap();

        let resolver = ReferenceResolver::new(dir.path());
        let result = resolver.resolve("Start @a.md middle @b.md end").unwrap();
        assert_eq!(result, "Start AAA middle BBB end");
    }

    #[test]
    fn blocks_path_traversal() {
        let dir = TempDir::new().unwrap();
        // Create a file outside the root to ensure it exists but is still blocked
        let parent = dir.path().parent().unwrap();
        let target = parent.join("secret.txt");
        fs::write(&target, "sensitive data").unwrap();

        let resolver = ReferenceResolver::new(dir.path());
        let result = resolver.resolve("@../secret.txt here");
        // Clean up
        let _ = fs::remove_file(&target);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("path traversal blocked"),
            "expected path traversal error, got: {err}"
        );
    }

    #[test]
    fn no_references_passes_through() {
        let dir = TempDir::new().unwrap();
        let resolver = ReferenceResolver::new(dir.path());
        let result = resolver.resolve("No refs here at all").unwrap();
        assert_eq!(result, "No refs here at all");
    }
}
