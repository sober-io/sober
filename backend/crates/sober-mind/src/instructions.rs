//! Instruction file types and loader for the structured prompt directory.
//!
//! Loads, caches, filters, and sorts instruction files from three layers:
//!
//! - **Base** (`sober-mind/prompt/`) — compiled into the binary via `include_str!()`
//! - **User** (`~/.sober/*.md`) — loaded once at startup
//! - **Workspace** (`.sober/*.md`) — lazily loaded per workspace

use std::path::Path;

use sober_core::types::access::TriggerKind;
use tracing::debug;

use crate::error::MindError;
use crate::frontmatter::{
    InstructionCategory, InstructionFrontmatter, Visibility, parse_frontmatter,
};

/// The filename that workspace layers are NOT allowed to extend.
const PROTECTED_FILE: &str = "safety.md";

/// A parsed instruction file ready for assembly.
#[derive(Debug, Clone)]
pub struct InstructionFile {
    /// Filename (e.g., `soul.md`).
    pub filename: String,
    /// Parsed frontmatter metadata.
    pub frontmatter: InstructionFrontmatter,
    /// Markdown body content (frontmatter stripped).
    pub body: String,
}

impl InstructionFile {
    /// Returns `true` if this instruction is visible to the given trigger kind.
    #[must_use]
    pub fn is_visible(&self, trigger: TriggerKind) -> bool {
        match self.frontmatter.visibility {
            Visibility::Public => true,
            Visibility::Internal => matches!(trigger, TriggerKind::Admin | TriggerKind::Scheduler),
        }
    }

    /// Sort key: `(category, priority, filename)`.
    fn sort_key(&self) -> (InstructionCategory, u32, &str) {
        (
            self.frontmatter.category,
            self.frontmatter.priority,
            &self.filename,
        )
    }
}

/// Filters instructions by visibility and sorts by category -> priority -> filename.
pub fn filter_and_sort(
    instructions: &[InstructionFile],
    trigger: TriggerKind,
) -> Vec<&InstructionFile> {
    let mut visible: Vec<&InstructionFile> = instructions
        .iter()
        .filter(|i| i.is_visible(trigger))
        .collect();
    visible.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    visible
}

// ---------------------------------------------------------------------------
// Embedded base instruction files (compiled into the binary)
// ---------------------------------------------------------------------------

/// Static entry: `(filename, raw content)`.
struct EmbeddedFile {
    filename: &'static str,
    content: &'static str,
}

/// All base instruction files, embedded at compile time.
const EMBEDDED_FILES: &[EmbeddedFile] = &[
    EmbeddedFile {
        filename: "soul.md",
        content: include_str!("../instructions/soul.md"),
    },
    EmbeddedFile {
        filename: "safety.md",
        content: include_str!("../instructions/safety.md"),
    },
    EmbeddedFile {
        filename: "memory.md",
        content: include_str!("../instructions/memory.md"),
    },
    EmbeddedFile {
        filename: "reasoning.md",
        content: include_str!("../instructions/reasoning.md"),
    },
    EmbeddedFile {
        filename: "evolution.md",
        content: include_str!("../instructions/evolution.md"),
    },
    EmbeddedFile {
        filename: "tools.md",
        content: include_str!("../instructions/tools.md"),
    },
    EmbeddedFile {
        filename: "workspace.md",
        content: include_str!("../instructions/workspace.md"),
    },
    EmbeddedFile {
        filename: "artifacts.md",
        content: include_str!("../instructions/artifacts.md"),
    },
    EmbeddedFile {
        filename: "extraction.md",
        content: include_str!("../instructions/extraction.md"),
    },
    EmbeddedFile {
        filename: "internal-tools.md",
        content: include_str!("../instructions/internal-tools.md"),
    },
];

// ---------------------------------------------------------------------------
// InstructionLoader — parse, load, merge
// ---------------------------------------------------------------------------

/// Loads and caches instruction files from base (embedded) and user layers.
///
/// Base files are compiled in. User files are loaded from `~/.sober/*.md` at
/// construction time and merged with base instructions. Workspace files are
/// loaded on-demand via [`load_workspace`](InstructionLoader::load_workspace).
#[derive(Debug)]
pub struct InstructionLoader {
    /// Pre-merged base + user instructions (immutable after construction).
    cached: Vec<InstructionFile>,
}

impl InstructionLoader {
    /// Creates a new loader: parses embedded base files, optionally loads and
    /// merges user layer files from `user_dir`.
    pub fn new(user_dir: Option<&Path>) -> Result<Self, MindError> {
        // 1. Parse embedded base files
        let mut instructions: Vec<InstructionFile> = EMBEDDED_FILES
            .iter()
            .map(|ef| parse_embedded(ef.filename, ef.content))
            .collect::<Result<Vec<_>, _>>()?;

        // 2. Load user layer files (if directory exists)
        if let Some(dir) = user_dir
            && dir.is_dir()
        {
            let user_files = load_directory(dir)?;
            merge_extensions(&mut instructions, user_files, false)?;
        }

        debug!(count = instructions.len(), "instruction files loaded");

        Ok(Self {
            cached: instructions,
        })
    }

    /// Returns the pre-cached base + user instructions.
    pub fn cached(&self) -> &[InstructionFile] {
        &self.cached
    }

    /// Loads workspace instruction files from the given directory.
    ///
    /// Returns standalone and extension files ready to be combined with
    /// `cached()`. Workspace files cannot extend `safety.md`.
    pub fn load_workspace(dir: &Path) -> Result<Vec<InstructionFile>, MindError> {
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        load_directory(dir)
    }

    /// Merges workspace instructions into a clone of cached base+user instructions.
    ///
    /// Extension files have their content appended to the matching base file.
    /// Standalone workspace files are added as new entries. Workspace files
    /// cannot extend `safety.md` (returns error).
    pub fn merge_with_workspace(
        &self,
        workspace_files: Vec<InstructionFile>,
    ) -> Result<Vec<InstructionFile>, MindError> {
        let mut merged = self.cached.clone();
        merge_extensions(&mut merged, workspace_files, true)?;
        Ok(merged)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parses an embedded `(filename, raw_content)` pair into an `InstructionFile`.
fn parse_embedded(filename: &str, content: &str) -> Result<InstructionFile, MindError> {
    let (fm, body) = parse_frontmatter(content)
        .map_err(|e| MindError::FrontmatterParseFailed(format!("{filename}: {e}")))?;

    Ok(InstructionFile {
        filename: filename.to_string(),
        frontmatter: fm,
        body: body.to_string(),
    })
}

/// Reads all `.md` files from a directory and parses them as instruction files.
fn load_directory(dir: &Path) -> Result<Vec<InstructionFile>, MindError> {
    let mut files = Vec::new();

    let entries = std::fs::read_dir(dir).map_err(|e| {
        MindError::InstructionLoadFailed(format!("cannot read directory {}: {e}", dir.display()))
    })?;

    for entry in entries {
        let entry = entry
            .map_err(|e| MindError::InstructionLoadFailed(format!("directory entry error: {e}")))?;

        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            let content = std::fs::read_to_string(&path).map_err(|e| {
                MindError::InstructionLoadFailed(format!("cannot read {}: {e}", path.display()))
            })?;

            let filename = path
                .file_name()
                .expect("path has filename")
                .to_string_lossy()
                .to_string();

            let (fm, body) = parse_frontmatter(&content)
                .map_err(|e| MindError::FrontmatterParseFailed(format!("{filename}: {e}")))?;

            files.push(InstructionFile {
                filename,
                frontmatter: fm,
                body: body.to_string(),
            });
        }
    }

    Ok(files)
}

/// Merges extension files into the base set.
///
/// - Files with `extends: <filename>` have their body appended to the named base file.
/// - Files without `extends` are added as standalone entries.
/// - If `is_workspace` is true, extending `safety.md` is forbidden.
fn merge_extensions(
    base: &mut Vec<InstructionFile>,
    layer_files: Vec<InstructionFile>,
    is_workspace: bool,
) -> Result<(), MindError> {
    for file in layer_files {
        if let Some(ref target) = file.frontmatter.extends {
            // Workspace cannot extend safety.md
            if is_workspace && target == PROTECTED_FILE {
                return Err(MindError::SoulMergeFailed(format!(
                    "workspace file '{}' cannot extend protected file '{PROTECTED_FILE}'",
                    file.filename
                )));
            }

            // Find the base file to extend
            if let Some(base_file) = base.iter_mut().find(|b| b.filename == *target) {
                base_file.body.push_str("\n\n");
                base_file.body.push_str(&file.body);
            } else {
                return Err(MindError::InstructionLoadFailed(format!(
                    "'{}' extends '{}' which does not exist",
                    file.filename, target
                )));
            }
        } else {
            // Standalone file — add to the set
            base.push(file);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_instruction(
        filename: &str,
        category: InstructionCategory,
        visibility: Visibility,
        priority: u32,
    ) -> InstructionFile {
        InstructionFile {
            filename: filename.to_string(),
            frontmatter: InstructionFrontmatter {
                category,
                visibility,
                priority,
                extends: None,
            },
            body: format!("Content of {filename}"),
        }
    }

    #[test]
    fn public_visible_to_all_triggers() {
        let inst = make_instruction(
            "soul.md",
            InstructionCategory::Personality,
            Visibility::Public,
            10,
        );
        assert!(inst.is_visible(TriggerKind::Human));
        assert!(inst.is_visible(TriggerKind::Replica));
        assert!(inst.is_visible(TriggerKind::Admin));
        assert!(inst.is_visible(TriggerKind::Scheduler));
    }

    #[test]
    fn internal_only_for_admin_and_scheduler() {
        let inst = make_instruction(
            "reasoning.md",
            InstructionCategory::Behavior,
            Visibility::Internal,
            20,
        );
        assert!(!inst.is_visible(TriggerKind::Human));
        assert!(!inst.is_visible(TriggerKind::Replica));
        assert!(inst.is_visible(TriggerKind::Admin));
        assert!(inst.is_visible(TriggerKind::Scheduler));
    }

    #[test]
    fn sorts_by_category_then_priority_then_filename() {
        let files = vec![
            make_instruction(
                "tools.md",
                InstructionCategory::Operation,
                Visibility::Public,
                10,
            ),
            make_instruction(
                "soul.md",
                InstructionCategory::Personality,
                Visibility::Public,
                10,
            ),
            make_instruction(
                "safety.md",
                InstructionCategory::Guardrail,
                Visibility::Public,
                10,
            ),
            make_instruction(
                "memory.md",
                InstructionCategory::Behavior,
                Visibility::Public,
                10,
            ),
            make_instruction(
                "workspace.md",
                InstructionCategory::Operation,
                Visibility::Public,
                20,
            ),
        ];

        let sorted = filter_and_sort(&files, TriggerKind::Scheduler);
        let names: Vec<&str> = sorted.iter().map(|i| i.filename.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "soul.md",
                "safety.md",
                "memory.md",
                "tools.md",
                "workspace.md"
            ]
        );
    }

    #[test]
    fn filters_out_internal_for_human() {
        let files = vec![
            make_instruction(
                "soul.md",
                InstructionCategory::Personality,
                Visibility::Public,
                10,
            ),
            make_instruction(
                "reasoning.md",
                InstructionCategory::Behavior,
                Visibility::Internal,
                20,
            ),
            make_instruction(
                "tools.md",
                InstructionCategory::Operation,
                Visibility::Public,
                10,
            ),
        ];

        let sorted = filter_and_sort(&files, TriggerKind::Human);
        let names: Vec<&str> = sorted.iter().map(|i| i.filename.as_str()).collect();
        assert_eq!(names, vec!["soul.md", "tools.md"]);
    }

    #[test]
    fn alphabetical_tiebreak_within_same_priority() {
        let files = vec![
            make_instruction(
                "b.md",
                InstructionCategory::Operation,
                Visibility::Public,
                10,
            ),
            make_instruction(
                "a.md",
                InstructionCategory::Operation,
                Visibility::Public,
                10,
            ),
        ];

        let sorted = filter_and_sort(&files, TriggerKind::Human);
        let names: Vec<&str> = sorted.iter().map(|i| i.filename.as_str()).collect();
        assert_eq!(names, vec!["a.md", "b.md"]);
    }

    // -----------------------------------------------------------------------
    // InstructionLoader tests
    // -----------------------------------------------------------------------

    #[test]
    fn loads_embedded_base_files() {
        let loader = InstructionLoader::new(None).unwrap();
        let files = loader.cached();
        assert_eq!(files.len(), 10);

        // Verify soul.md is present with correct metadata
        let soul = files.iter().find(|f| f.filename == "soul.md").unwrap();
        assert_eq!(soul.frontmatter.category, InstructionCategory::Personality);
        assert_eq!(soul.frontmatter.visibility, Visibility::Public);
        assert!(soul.body.contains("Sõber"));
    }

    #[test]
    fn loads_user_extension_files() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("extra-memory.md"),
            "---\ncategory: behavior\nextends: memory.md\n---\nUser memory extension.",
        )
        .unwrap();

        let loader = InstructionLoader::new(Some(dir.path())).unwrap();
        let memory = loader
            .cached()
            .iter()
            .find(|f| f.filename == "memory.md")
            .unwrap();
        assert!(memory.body.contains("User memory extension."));
    }

    #[test]
    fn loads_user_standalone_files() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("custom.md"),
            "---\ncategory: behavior\npriority: 15\n---\nCustom user behavior.",
        )
        .unwrap();

        let loader = InstructionLoader::new(Some(dir.path())).unwrap();
        assert_eq!(loader.cached().len(), 11); // 10 base + 1 standalone
        let custom = loader
            .cached()
            .iter()
            .find(|f| f.filename == "custom.md")
            .unwrap();
        assert!(custom.body.contains("Custom user behavior."));
    }

    #[test]
    fn missing_user_dir_is_fine() {
        let loader = InstructionLoader::new(Some(Path::new("/nonexistent/dir"))).unwrap();
        assert_eq!(loader.cached().len(), 10);
    }

    #[test]
    fn workspace_cannot_extend_safety() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("evil.md"),
            "---\ncategory: guardrail\nextends: safety.md\n---\nOverride safety.",
        )
        .unwrap();

        let loader = InstructionLoader::new(None).unwrap();
        let ws_files = InstructionLoader::load_workspace(dir.path()).unwrap();
        let result = loader.merge_with_workspace(ws_files);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("protected file"));
    }

    #[test]
    fn workspace_standalone_files_merge() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("project.md"),
            "---\ncategory: behavior\npriority: 25\n---\nProject-specific behavior.",
        )
        .unwrap();

        let loader = InstructionLoader::new(None).unwrap();
        let ws_files = InstructionLoader::load_workspace(dir.path()).unwrap();
        let merged = loader.merge_with_workspace(ws_files).unwrap();
        assert_eq!(merged.len(), 11);
    }

    #[test]
    fn workspace_extension_appends_to_base() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("extra-tools.md"),
            "---\ncategory: operation\nextends: tools.md\n---\nWorkspace tool additions.",
        )
        .unwrap();

        let loader = InstructionLoader::new(None).unwrap();
        let ws_files = InstructionLoader::load_workspace(dir.path()).unwrap();
        let merged = loader.merge_with_workspace(ws_files).unwrap();
        let tools = merged.iter().find(|f| f.filename == "tools.md").unwrap();
        assert!(tools.body.contains("Workspace tool additions."));
    }

    #[test]
    fn extension_to_nonexistent_file_errors() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("bad.md"),
            "---\ncategory: behavior\nextends: nonexistent.md\n---\nBad ref.",
        )
        .unwrap();

        let loader = InstructionLoader::new(Some(dir.path()));
        assert!(loader.is_err());
        assert!(loader.unwrap_err().to_string().contains("does not exist"));
    }
}
