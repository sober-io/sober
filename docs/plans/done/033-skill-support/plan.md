# #033 — Skill Support Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development
> (if subagents available) or superpowers:executing-plans to implement this plan.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Agent Skills spec-compliant skill discovery, loading,
activation, and slash command registration.

**Architecture:** New `sober-skill` library crate handles filesystem scanning,
YAML frontmatter parsing, and catalog management. The `activate_skill` tool
lets the LLM load skills on demand. The API proxies skill listing via gRPC
to the agent. Frontend fetches skill list for slash command autocomplete.

**Tech Stack:** Rust (serde_yml, tokio::fs), tonic/prost (gRPC), SvelteKit
(TypeScript)

---

## File Map

### New files (sober-skill crate)

| File | Responsibility |
|------|---------------|
| `backend/crates/sober-skill/Cargo.toml` | Crate manifest |
| `backend/crates/sober-skill/src/lib.rs` | Module declarations + re-exports |
| `backend/crates/sober-skill/src/error.rs` | `SkillError` enum + `From<SkillError> for AppError` |
| `backend/crates/sober-skill/src/frontmatter.rs` | `SkillFrontmatter` parsing from YAML |
| `backend/crates/sober-skill/src/types.rs` | `SkillEntry`, `SkillSource`, `SkillActivationState` |
| `backend/crates/sober-skill/src/loader.rs` | `SkillLoader` — scan dirs, parse, cache with TTL |
| `backend/crates/sober-skill/src/catalog.rs` | `SkillCatalog` — immutable index, prompt generation |
| `backend/crates/sober-skill/src/tool.rs` | `ActivateSkillTool` — implements `Tool` trait |

### Modified files

| File | Change |
|------|--------|
| `backend/proto/sober/agent/v1/agent.proto` | Add `ListSkills` RPC |
| `backend/crates/sober-agent/Cargo.toml` | Add `sober-skill` dependency |
| `backend/crates/sober-agent/src/grpc.rs` | Implement `ListSkills` RPC |
| `backend/crates/sober-agent/src/tools/bootstrap.rs` | Register `ActivateSkillTool` |
| `backend/crates/sober-mind/Cargo.toml` | Add `sober-skill` dependency |
| `backend/crates/sober-mind/src/assembly.rs` | Inject skill catalog into system prompt |
| `backend/crates/sober-api/src/routes/mod.rs` | Register skills routes |
| `backend/crates/sober-api/src/routes/skills.rs` | New `GET /api/v1/skills` handler |
| `frontend/src/lib/services/skills.ts` | New skills API service |
| `frontend/src/lib/types/index.ts` | Add `SkillInfo` type |

### Test fixtures

| File | Purpose |
|------|---------|
| `backend/crates/sober-skill/tests/fixtures/valid-skill/SKILL.md` | Valid skill |
| `backend/crates/sober-skill/tests/fixtures/with-scripts/SKILL.md` | Skill with scripts/ dir |
| `backend/crates/sober-skill/tests/fixtures/with-scripts/scripts/helper.sh` | Script resource |
| `backend/crates/sober-skill/tests/fixtures/malformed-yaml/SKILL.md` | Broken frontmatter |
| `backend/crates/sober-skill/tests/fixtures/missing-description/SKILL.md` | Missing required field |
| `backend/crates/sober-skill/tests/fixtures/bad-name/SKILL.md` | Invalid name format |

---

## Chunk 1: sober-skill Crate Foundation

### Task 1: Create crate scaffold

**Files:**
- Create: `backend/crates/sober-skill/Cargo.toml`
- Create: `backend/crates/sober-skill/src/lib.rs`
- Create: `backend/crates/sober-skill/src/error.rs`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "sober-skill"
version = "0.1.0"
edition.workspace = true

[dependencies]
sober-core = { path = "../sober-core" }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
serde_yml = "0.0.12"

[dev-dependencies]
tempfile = "3.26.0"
```

- [ ] **Step 2: Create error.rs**

```rust
//! Skill-specific error types.

use sober_core::error::AppError;

/// Errors that can occur during skill operations.
#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    /// Skill not found in catalog.
    #[error("Skill not found: {0}")]
    NotFound(String),

    /// Failed to parse SKILL.md frontmatter.
    #[error("Failed to parse SKILL.md frontmatter in {path}: {reason}")]
    FrontmatterParseFailed { path: String, reason: String },

    /// Failed to read skill file or directory.
    #[error("Failed to read skill: {0}")]
    IoError(#[from] std::io::Error),

    /// Skill already activated in this conversation.
    #[error("Skill already active: {0}")]
    AlreadyActive(String),
}

impl From<SkillError> for AppError {
    fn from(err: SkillError) -> Self {
        match err {
            SkillError::NotFound(msg) => AppError::NotFound(msg),
            SkillError::AlreadyActive(msg) => AppError::Conflict(msg),
            SkillError::FrontmatterParseFailed { path, reason } => {
                AppError::Internal(Box::new(std::io::Error::other(
                    format!("skill frontmatter parse failed in {path}: {reason}"),
                )))
            }
            SkillError::IoError(e) => AppError::Internal(Box::new(e)),
        }
    }
}
```

- [ ] **Step 3: Create lib.rs (minimal)**

```rust
//! Agent Skills spec-compliant skill discovery, loading, and activation.

pub mod error;

pub use error::SkillError;
```

- [ ] **Step 4: Verify it compiles**

Run: `cd backend && cargo build -q -p sober-skill`
Expected: Success

- [ ] **Step 5: Move plan to active**

```bash
git mv docs/plans/pending/033-skill-support docs/plans/active/033-skill-support
```

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-skill/ docs/plans/
git commit -m "feat(skill): scaffold sober-skill crate with error types"
```

---

### Task 2: Frontmatter parsing

**Files:**
- Create: `backend/crates/sober-skill/src/frontmatter.rs`
- Modify: `backend/crates/sober-skill/src/lib.rs`

- [ ] **Step 1: Write frontmatter parsing tests**

Add to `frontmatter.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_frontmatter() {
        let content = "---\nname: my-skill\ndescription: A test skill.\n---\n\n## Body";
        let (fm, body) = parse_skill_frontmatter(content).unwrap();
        assert_eq!(fm.name, "my-skill");
        assert_eq!(fm.description, "A test skill.");
        assert_eq!(body, "## Body");
        assert!(fm.license.is_none());
        assert!(fm.metadata.is_none());
    }

    #[test]
    fn parses_full_frontmatter() {
        let content = "\
---
name: pdf-processing
description: Extract PDF text and fill forms.
license: Apache-2.0
compatibility: Requires poppler-utils
metadata:
  author: test-org
  version: '1.0'
allowed-tools: Bash(git:*) Read
---

Instructions here.";
        let (fm, body) = parse_skill_frontmatter(content).unwrap();
        assert_eq!(fm.name, "pdf-processing");
        assert_eq!(fm.license.as_deref(), Some("Apache-2.0"));
        assert_eq!(fm.compatibility.as_deref(), Some("Requires poppler-utils"));
        assert_eq!(fm.allowed_tools, Some(vec!["Bash(git:*)".into(), "Read".into()]));
        let meta = fm.metadata.as_ref().unwrap();
        assert_eq!(meta.get("author").unwrap(), "test-org");
        assert_eq!(body, "Instructions here.");
    }

    #[test]
    fn rejects_missing_description() {
        let content = "---\nname: no-desc\n---\nBody";
        let err = parse_skill_frontmatter(content).unwrap_err();
        assert!(matches!(err, SkillError::FrontmatterParseFailed { .. }));
    }

    #[test]
    fn rejects_empty_description() {
        let content = "---\nname: empty\ndescription: ''\n---\nBody";
        let err = parse_skill_frontmatter(content).unwrap_err();
        assert!(matches!(err, SkillError::FrontmatterParseFailed { .. }));
    }

    #[test]
    fn rejects_unparseable_yaml() {
        let content = "---\n: invalid: yaml: [[\n---\nBody";
        let err = parse_skill_frontmatter(content).unwrap_err();
        assert!(matches!(err, SkillError::FrontmatterParseFailed { .. }));
    }

    #[test]
    fn validates_name_format() {
        // Valid names
        assert!(validate_skill_name("my-skill").is_empty());
        assert!(validate_skill_name("a").is_empty());
        assert!(validate_skill_name("code-review").is_empty());

        // Invalid names — return warnings, don't reject
        assert!(!validate_skill_name("My-Skill").is_empty());     // uppercase
        assert!(!validate_skill_name("-leading").is_empty());      // leading hyphen
        assert!(!validate_skill_name("trailing-").is_empty());     // trailing hyphen
        assert!(!validate_skill_name("double--hyphen").is_empty()); // consecutive
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && cargo test -p sober-skill -q`
Expected: FAIL — functions not defined

- [ ] **Step 3: Implement frontmatter parsing**

```rust
//! Agent Skills spec-compliant SKILL.md frontmatter parsing.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::SkillError;

/// Parsed SKILL.md frontmatter following the Agent Skills specification.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillFrontmatter {
    /// Skill name (required). 1-64 chars, lowercase alphanumeric + hyphens.
    pub name: String,
    /// What the skill does and when to use it (required). Max 1024 chars.
    pub description: String,
    /// License name or reference to bundled license file.
    pub license: Option<String>,
    /// Environment requirements (max 500 chars).
    pub compatibility: Option<String>,
    /// Arbitrary key-value metadata.
    pub metadata: Option<BTreeMap<String, String>>,
    /// Pre-approved tools (space-delimited in YAML, parsed to Vec).
    #[serde(default, deserialize_with = "deserialize_allowed_tools")]
    #[serde(rename = "allowed-tools")]
    pub allowed_tools: Option<Vec<String>>,
}

/// Deserializes the `allowed-tools` field from a space-delimited string.
fn deserialize_allowed_tools<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt.map(|s| s.split_whitespace().map(String::from).collect()))
}

/// Validates a skill name per the Agent Skills spec.
/// Returns a list of warnings (empty = valid).
pub fn validate_skill_name(name: &str) -> Vec<String> {
    let mut warnings = Vec::new();

    if name.is_empty() || name.len() > 64 {
        warnings.push(format!("name length {} outside 1-64 range", name.len()));
    }
    if name.starts_with('-') || name.ends_with('-') {
        warnings.push("name must not start or end with a hyphen".into());
    }
    if name.contains("--") {
        warnings.push("name must not contain consecutive hyphens".into());
    }
    if name.chars().any(|c| !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-') {
        warnings.push("name must contain only lowercase alphanumeric characters and hyphens".into());
    }

    warnings
}

/// Parses a SKILL.md file into frontmatter and body.
///
/// Expects YAML frontmatter between `---` delimiters followed by markdown.
/// Returns `(frontmatter, body)` where body is trimmed.
pub fn parse_skill_frontmatter(content: &str) -> Result<(SkillFrontmatter, &str), SkillError> {
    let content = content.trim_start();

    // Find opening ---
    if !content.starts_with("---") {
        return Err(SkillError::FrontmatterParseFailed {
            path: String::new(),
            reason: "missing opening --- delimiter".into(),
        });
    }

    // Find closing ---
    let after_open = &content[3..].trim_start_matches(['\r', '\n']);
    let closing = after_open.find("\n---").ok_or_else(|| SkillError::FrontmatterParseFailed {
        path: String::new(),
        reason: "missing closing --- delimiter".into(),
    })?;

    let yaml_str = &after_open[..closing];
    let body_start = closing + 4; // skip \n---
    let body = after_open[body_start..].trim();

    // Parse YAML
    let fm: SkillFrontmatter = serde_yml::from_str(yaml_str).map_err(|e| {
        SkillError::FrontmatterParseFailed {
            path: String::new(),
            reason: e.to_string(),
        }
    })?;

    // Validate required fields
    if fm.description.trim().is_empty() {
        return Err(SkillError::FrontmatterParseFailed {
            path: String::new(),
            reason: "description must not be empty".into(),
        });
    }

    Ok((fm, body))
}
```

- [ ] **Step 4: Update lib.rs**

```rust
pub mod error;
pub mod frontmatter;

pub use error::SkillError;
pub use frontmatter::{SkillFrontmatter, parse_skill_frontmatter, validate_skill_name};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd backend && cargo test -p sober-skill -q`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-skill/
git commit -m "feat(skill): implement Agent Skills spec frontmatter parsing"
```

---

### Task 3: Types and test fixtures

**Files:**
- Create: `backend/crates/sober-skill/src/types.rs`
- Create: `backend/crates/sober-skill/tests/fixtures/valid-skill/SKILL.md`
- Create: `backend/crates/sober-skill/tests/fixtures/with-scripts/SKILL.md`
- Create: `backend/crates/sober-skill/tests/fixtures/with-scripts/scripts/helper.sh`
- Create: `backend/crates/sober-skill/tests/fixtures/malformed-yaml/SKILL.md`
- Create: `backend/crates/sober-skill/tests/fixtures/missing-description/SKILL.md`
- Create: `backend/crates/sober-skill/tests/fixtures/bad-name/SKILL.md`
- Modify: `backend/crates/sober-skill/src/lib.rs`

- [ ] **Step 1: Create types.rs**

```rust
//! Core types for skill discovery and activation.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::frontmatter::SkillFrontmatter;

/// Where a skill was discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    /// User-level: `~/.sober/skills/` or `~/.agents/skills/`
    User,
    /// Workspace-level: `.sober/skills/` or `.agents/skills/`
    Workspace,
}

/// A discovered skill entry in the catalog.
#[derive(Debug, Clone)]
pub struct SkillEntry {
    /// Parsed SKILL.md frontmatter.
    pub frontmatter: SkillFrontmatter,
    /// Absolute path to the SKILL.md file.
    pub path: PathBuf,
    /// Skill directory root (parent of SKILL.md).
    pub base_dir: PathBuf,
    /// Where this skill was discovered.
    pub source: SkillSource,
}

/// Per-conversation activation tracking.
///
/// Created fresh for each conversation, discarded when it ends.
/// Prevents duplicate skill injection within a single conversation.
#[derive(Debug, Default)]
pub struct SkillActivationState {
    activated: HashSet<String>,
}

impl SkillActivationState {
    /// Returns `true` if the skill has already been activated.
    pub fn is_activated(&self, name: &str) -> bool {
        self.activated.contains(name)
    }

    /// Marks a skill as activated. Returns `false` if already active.
    pub fn activate(&mut self, name: String) -> bool {
        self.activated.insert(name)
    }
}
```

- [ ] **Step 2: Create test fixtures**

`tests/fixtures/valid-skill/SKILL.md`:
```markdown
---
name: valid-skill
description: A valid test skill for unit testing.
metadata:
  author: test
  version: "1.0"
---

## Instructions

This is a valid skill body.
```

`tests/fixtures/with-scripts/SKILL.md`:
```markdown
---
name: with-scripts
description: A skill that includes scripts and references.
---

## Instructions

Run the helper script:

```bash
bash scripts/helper.sh
```
```

`tests/fixtures/with-scripts/scripts/helper.sh`:
```bash
#!/bin/bash
echo "helper script"
```

`tests/fixtures/malformed-yaml/SKILL.md`:
```markdown
---
name: [invalid yaml
description: broken
---

Body.
```

`tests/fixtures/missing-description/SKILL.md`:
```markdown
---
name: missing-description
---

Body without description.
```

`tests/fixtures/bad-name/SKILL.md`:
```markdown
---
name: Bad--Name
description: Has invalid name format.
---

Body.
```

- [ ] **Step 3: Update lib.rs**

```rust
pub mod error;
pub mod frontmatter;
pub mod types;

pub use error::SkillError;
pub use frontmatter::{SkillFrontmatter, parse_skill_frontmatter, validate_skill_name};
pub use types::{SkillActivationState, SkillEntry, SkillSource};
```

- [ ] **Step 4: Verify it compiles**

Run: `cd backend && cargo build -q -p sober-skill`
Expected: Success

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-skill/
git commit -m "feat(skill): add core types and test fixtures"
```

---

### Task 4: SkillCatalog — immutable index and prompt generation

**Note:** Catalog must be created before Loader because Loader returns a
`SkillCatalog`. See Task 5 (originally Task 5, now after this task).

**Files:**
- Create: `backend/crates/sober-skill/src/catalog.rs`
- Modify: `backend/crates/sober-skill/src/lib.rs`

- [ ] **Step 1: Write catalog tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::SkillFrontmatter;
    use crate::types::{SkillEntry, SkillSource};
    use std::path::PathBuf;

    fn test_entry(name: &str, description: &str) -> SkillEntry {
        SkillEntry {
            frontmatter: SkillFrontmatter {
                name: name.to_string(),
                description: description.to_string(),
                license: None,
                compatibility: None,
                metadata: None,
                allowed_tools: None,
            },
            path: PathBuf::from(format!("/skills/{name}/SKILL.md")),
            base_dir: PathBuf::from(format!("/skills/{name}")),
            source: SkillSource::User,
        }
    }

    #[test]
    fn catalog_lookup() {
        let mut skills = HashMap::new();
        skills.insert("code-review".into(), test_entry("code-review", "Reviews code."));
        let catalog = SkillCatalog::new(skills);

        assert!(catalog.get("code-review").is_some());
        assert!(catalog.get("nonexistent").is_none());
    }

    #[test]
    fn catalog_names_returns_sorted() {
        let mut skills = HashMap::new();
        skills.insert("zebra".into(), test_entry("zebra", "Z skill."));
        skills.insert("alpha".into(), test_entry("alpha", "A skill."));
        let catalog = SkillCatalog::new(skills);

        assert_eq!(catalog.names(), vec!["alpha", "zebra"]);
    }

    #[test]
    fn catalog_xml_generation() {
        let mut skills = HashMap::new();
        skills.insert("my-skill".into(), test_entry("my-skill", "Does things."));
        let catalog = SkillCatalog::new(skills);

        let xml = catalog.to_catalog_xml();
        assert!(xml.contains("<available_skills>"));
        assert!(xml.contains("name=\"my-skill\""));
        assert!(xml.contains("Does things."));
        assert!(xml.contains("</available_skills>"));
    }

    #[test]
    fn empty_catalog_returns_empty_string() {
        let catalog = SkillCatalog::new(HashMap::new());
        assert!(catalog.to_catalog_xml().is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && cargo test -p sober-skill -q`
Expected: FAIL

- [ ] **Step 3: Implement SkillCatalog**

```rust
//! Immutable skill catalog — provides lookup and prompt generation.

use std::collections::HashMap;

use crate::types::SkillEntry;

/// Immutable index of discovered skills.
///
/// Built by [`SkillLoader`](crate::loader::SkillLoader) from filesystem
/// scanning. Provides lookup by name and generates the XML catalog block
/// for system prompt injection.
#[derive(Debug)]
pub struct SkillCatalog {
    skills: HashMap<String, SkillEntry>,
}

impl SkillCatalog {
    /// Creates a new catalog from a map of skill entries.
    pub fn new(skills: HashMap<String, SkillEntry>) -> Self {
        Self { skills }
    }

    /// Returns a skill entry by name.
    pub fn get(&self, name: &str) -> Option<&SkillEntry> {
        self.skills.get(name)
    }

    /// Returns sorted list of all skill names.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.skills.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    /// Returns true if the catalog has no skills.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Returns the number of skills in the catalog.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Generates the `<available_skills>` XML block for system prompt injection.
    ///
    /// Returns an empty string if there are no skills. The block includes
    /// behavioral instructions for the LLM.
    pub fn to_catalog_xml(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }

        let mut xml = String::new();
        xml.push_str("The following skills provide specialized instructions for specific tasks.\n");
        xml.push_str("When a task matches a skill's description, call the activate_skill tool\n");
        xml.push_str("with the skill's name to load its full instructions.\n\n");
        xml.push_str("<available_skills>\n");

        let mut entries: Vec<_> = self.skills.iter().collect();
        entries.sort_by_key(|(name, _)| name.as_str());

        for (_, entry) in entries {
            xml.push_str(&format!(
                "  <skill name=\"{}\" path=\"{}\">\n    {}\n  </skill>\n",
                entry.frontmatter.name,
                entry.path.display(),
                entry.frontmatter.description.trim(),
            ));
        }

        xml.push_str("</available_skills>");
        xml
    }
}
```

- [ ] **Step 4: Update lib.rs**

Add `pub mod catalog;` and re-export `pub use catalog::SkillCatalog;`.

- [ ] **Step 5: Run tests**

Run: `cd backend && cargo test -p sober-skill -q`
Expected: All pass

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-skill/
git commit -m "feat(skill): implement SkillCatalog with lookup and prompt generation"
```

---

### Task 5: SkillLoader — directory scanning and caching

**Files:**
- Create: `backend/crates/sober-skill/src/loader.rs`
- Modify: `backend/crates/sober-skill/src/lib.rs`

- [ ] **Step 1: Write loader tests**

Add to `loader.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Creates a temp directory structure that mimics ~/.sober/skills/
    /// containing the test fixtures.
    fn setup_user_home() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join(".sober/skills");
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");

        // Copy fixture skills into the .sober/skills/ structure
        std::fs::create_dir_all(&skills_dir).unwrap();
        for entry in std::fs::read_dir(&fixtures).unwrap() {
            let entry = entry.unwrap();
            if entry.path().is_dir() {
                let dest = skills_dir.join(entry.file_name());
                copy_dir_recursive(&entry.path(), &dest);
            }
        }

        (tmp, tmp.path().to_path_buf())
    }

    fn copy_dir_recursive(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let dest = dst.join(entry.file_name());
            if entry.path().is_dir() {
                copy_dir_recursive(&entry.path(), &dest);
            } else {
                std::fs::copy(entry.path(), dest).unwrap();
            }
        }
    }

    #[tokio::test]
    async fn loads_valid_skills_from_directory() {
        let (_tmp, user_home) = setup_user_home();
        let loader = SkillLoader::new(Duration::from_secs(300));
        let catalog = loader
            .load(&user_home, &PathBuf::from("/nonexistent"))
            .await
            .unwrap();

        // Should find valid-skill and with-scripts (malformed/missing are skipped)
        assert!(catalog.get("valid-skill").is_some());
        assert!(catalog.get("with-scripts").is_some());
    }

    #[tokio::test]
    async fn skips_malformed_frontmatter() {
        let (_tmp, user_home) = setup_user_home();
        let loader = SkillLoader::new(Duration::from_secs(300));
        let catalog = loader
            .load(&user_home, &PathBuf::from("/nonexistent"))
            .await
            .unwrap();

        assert!(catalog.get("malformed-yaml").is_none());
    }

    #[tokio::test]
    async fn skips_missing_description() {
        let (_tmp, user_home) = setup_user_home();
        let loader = SkillLoader::new(Duration::from_secs(300));
        let catalog = loader
            .load(&user_home, &PathBuf::from("/nonexistent"))
            .await
            .unwrap();

        assert!(catalog.get("missing-description").is_none());
    }

    #[tokio::test]
    async fn warns_on_bad_name() {
        // bad-name has "Bad--Name" — should warn but still load
        let (_tmp, user_home) = setup_user_home();
        let loader = SkillLoader::new(Duration::from_secs(300));
        let catalog = loader
            .load(&user_home, &PathBuf::from("/nonexistent"))
            .await
            .unwrap();

        // Loaded with the frontmatter name (Bad--Name), warnings logged
        assert!(catalog.get("Bad--Name").is_some());
    }

    #[tokio::test]
    async fn caches_results_within_ttl() {
        let (_tmp, user_home) = setup_user_home();
        let loader = SkillLoader::new(Duration::from_secs(300));
        let empty = PathBuf::from("/nonexistent");

        let cat1 = loader.load(&user_home, &empty).await.unwrap();
        let cat2 = loader.load(&user_home, &empty).await.unwrap();

        // Should return the same Arc (cached)
        assert!(Arc::ptr_eq(&cat1, &cat2));
    }

    #[tokio::test]
    async fn workspace_overrides_user_on_collision() {
        // Create temp dirs simulating user and workspace with same skill name
        let tmp = tempfile::tempdir().unwrap();
        let user_dir = tmp.path().join("user-skills");
        let ws_dir = tmp.path().join("ws-skills");

        // User skill
        let user_skill = user_dir.join("my-skill");
        std::fs::create_dir_all(&user_skill).unwrap();
        std::fs::write(
            user_skill.join("SKILL.md"),
            "---\nname: my-skill\ndescription: User version.\n---\nUser body.",
        ).unwrap();

        // Workspace skill (same name)
        let ws_skill = ws_dir.join("my-skill");
        std::fs::create_dir_all(&ws_skill).unwrap();
        std::fs::write(
            ws_skill.join("SKILL.md"),
            "---\nname: my-skill\ndescription: Workspace version.\n---\nWS body.",
        ).unwrap();

        let loader = SkillLoader::new(Duration::from_secs(0)); // no cache
        let catalog = loader.load(&user_dir, &ws_dir).await.unwrap();

        let entry = catalog.get("my-skill").unwrap();
        assert_eq!(entry.source, SkillSource::Workspace);
        assert_eq!(entry.frontmatter.description, "Workspace version.");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && cargo test -p sober-skill -q`
Expected: FAIL — `SkillLoader` not defined

- [ ] **Step 3: Implement SkillLoader**

```rust
//! Filesystem-based skill discovery with TTL caching.
//!
//! Scans user-level and workspace-level directories for SKILL.md files,
//! parses frontmatter, and builds a [`SkillCatalog`]. Results are cached
//! per (user_home, workspace_path) key with a configurable TTL.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use tokio::fs;
use tracing::{debug, warn};

use crate::catalog::SkillCatalog;
use crate::frontmatter::{parse_skill_frontmatter, validate_skill_name};
use crate::types::{SkillEntry, SkillSource};
use crate::SkillError;

/// Maximum directory depth when scanning for skills.
const MAX_SCAN_DEPTH: usize = 4;

/// Maximum number of directories to scan.
const MAX_SCAN_DIRS: usize = 2000;

/// Cached catalog entry.
struct CachedCatalog {
    catalog: Arc<SkillCatalog>,
    loaded_at: Instant,
}

/// Discovers and caches skills from the filesystem.
pub struct SkillLoader {
    cache: RwLock<HashMap<(PathBuf, PathBuf), CachedCatalog>>,
    ttl: Duration,
}

impl SkillLoader {
    /// Creates a new loader with the given cache TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Returns a skill catalog for the given user home and workspace root.
    ///
    /// - `user_home`: the user's home directory (e.g., `/home/user`).
    ///   Scans `~/.sober/skills/` and `~/.agents/skills/`.
    /// - `workspace_root`: the project root directory (e.g., `/home/user/project`).
    ///   Scans `.sober/skills/` and `.agents/skills/` relative to it.
    ///
    /// Returns a cached catalog if one exists and is within TTL.
    pub async fn load(
        &self,
        user_home: &Path,
        workspace_root: &Path,
    ) -> Result<Arc<SkillCatalog>, SkillError> {
        let key = (user_home.to_path_buf(), workspace_path.to_path_buf());

        // Check cache (read lock)
        {
            let cache = self.cache.read().map_err(|_| {
                SkillError::IoError(std::io::Error::other("skill cache lock poisoned"))
            })?;
            if let Some(cached) = cache.get(&key) {
                if cached.loaded_at.elapsed() < self.ttl {
                    return Ok(Arc::clone(&cached.catalog));
                }
            }
        }

        // Cache miss or expired — rescan
        let catalog = Arc::new(self.scan(user_home, workspace_path).await?);

        // Update cache (write lock)
        {
            let mut cache = self.cache.write().map_err(|_| {
                SkillError::IoError(std::io::Error::other("skill cache lock poisoned"))
            })?;
            cache.insert(
                key,
                CachedCatalog {
                    catalog: Arc::clone(&catalog),
                    loaded_at: Instant::now(),
                },
            );
        }

        Ok(catalog)
    }

    /// Scans user and workspace directories for skills.
    ///
    /// `user_home` = `~` (e.g., `/home/user`)
    /// `workspace_root` = project root (e.g., `/home/user/project`)
    async fn scan(
        &self,
        user_home: &Path,
        workspace_root: &Path,
    ) -> Result<SkillCatalog, SkillError> {
        let mut skills = HashMap::new();

        // Scan user-level directories first (lower priority)
        let user_sober_skills = user_home.join(".sober/skills");
        let user_agents_skills = user_home.join(".agents/skills");

        for dir in [&user_sober_skills, &user_agents_skills] {
            if dir.is_dir() {
                self.scan_directory(dir, SkillSource::User, &mut skills).await;
            }
        }

        // Scan workspace-level directories (higher priority — overwrites user)
        let ws_sober_skills = workspace_root.join(".sober/skills");
        let ws_agents_skills = workspace_root.join(".agents/skills");

        for dir in [&ws_sober_skills, &ws_agents_skills] {
            if dir.is_dir() {
                self.scan_directory(dir, SkillSource::Workspace, &mut skills).await;
            }
        }

        debug!("loaded {} skills", skills.len());
        Ok(SkillCatalog::new(skills))
    }

    /// Scans a single directory for skill subdirectories.
    async fn scan_directory(
        &self,
        dir: &Path,
        source: SkillSource,
        skills: &mut HashMap<String, SkillEntry>,
    ) {
        let mut dirs_scanned = 0;

        let Ok(mut entries) = fs::read_dir(dir).await else {
            warn!(?dir, "failed to read skill directory");
            return;
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            if dirs_scanned >= MAX_SCAN_DIRS {
                warn!(?dir, "hit max directory scan limit");
                break;
            }

            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            // Skip common non-skill directories
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') || name_str == "node_modules" {
                continue;
            }

            dirs_scanned += 1;

            let skill_md = path.join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }

            match self.load_skill(&skill_md, &path, source).await {
                Ok(entry_val) => {
                    let skill_name = entry_val.frontmatter.name.clone();
                    if skills.contains_key(&skill_name) && source == SkillSource::User {
                        // User skill won't override existing (workspace has priority)
                        debug!(skill_name, "skill already loaded from higher-priority scope");
                        continue;
                    }
                    if skills.contains_key(&skill_name) {
                        warn!(skill_name, "skill name collision — overriding with {:?}", source);
                    }
                    skills.insert(skill_name, entry_val);
                }
                Err(e) => {
                    warn!(?skill_md, error = %e, "skipping skill");
                }
            }
        }
    }

    /// Loads and validates a single SKILL.md file.
    async fn load_skill(
        &self,
        skill_md: &Path,
        base_dir: &Path,
        source: SkillSource,
    ) -> Result<SkillEntry, SkillError> {
        let content = fs::read_to_string(skill_md).await?;
        let (frontmatter, _body) = parse_skill_frontmatter(&content).map_err(|e| {
            SkillError::FrontmatterParseFailed {
                path: skill_md.display().to_string(),
                reason: e.to_string(),
            }
        })?;

        // Validate name (warn, don't reject)
        let warnings = validate_skill_name(&frontmatter.name);
        for w in &warnings {
            warn!(skill = %frontmatter.name, path = ?skill_md, "name validation: {w}");
        }

        // Check name matches directory name (warn, don't reject)
        if let Some(dir_name) = base_dir.file_name().and_then(|n| n.to_str()) {
            if dir_name != frontmatter.name {
                warn!(
                    skill = %frontmatter.name,
                    dir = dir_name,
                    "skill name does not match directory name"
                );
            }
        }

        Ok(SkillEntry {
            frontmatter,
            path: skill_md.to_path_buf(),
            base_dir: base_dir.to_path_buf(),
            source,
        })
    }
}
```

- [ ] **Step 4: Update lib.rs to add loader module**

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd backend && cargo test -p sober-skill -q`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-skill/
git commit -m "feat(skill): implement SkillLoader with filesystem scanning and TTL cache"
```

---

### Task 6: ActivateSkillTool

**Files:**
- Create: `backend/crates/sober-skill/src/tool.rs`
- Modify: `backend/crates/sober-skill/src/lib.rs`

- [ ] **Step 1: Write tool tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::SkillFrontmatter;
    use crate::types::{SkillActivationState, SkillEntry, SkillSource};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn test_catalog() -> Arc<SkillCatalog> {
        // Create a temp skill file for reading
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_md = skill_dir.join("SKILL.md");
        std::fs::write(
            &skill_md,
            "---\nname: test-skill\ndescription: A test skill.\n---\n\n## Instructions\n\nDo the thing.",
        ).unwrap();

        let mut skills = HashMap::new();
        skills.insert(
            "test-skill".into(),
            SkillEntry {
                frontmatter: SkillFrontmatter {
                    name: "test-skill".into(),
                    description: "A test skill.".into(),
                    license: None,
                    compatibility: None,
                    metadata: None,
                    allowed_tools: None,
                },
                path: skill_md,
                base_dir: skill_dir,
                source: SkillSource::User,
            },
        );

        // Leak tempdir so files survive the test
        std::mem::forget(tmp);

        Arc::new(SkillCatalog::new(skills))
    }

    #[tokio::test]
    async fn activate_returns_skill_content() {
        let catalog = test_catalog();
        let state = Arc::new(Mutex::new(SkillActivationState::default()));
        let tool = ActivateSkillTool::new(Arc::clone(&catalog), Arc::clone(&state));

        let input = serde_json::json!({ "name": "test-skill" });
        let output = tool.execute_inner(input).await.unwrap();

        assert!(!output.is_error);
        assert!(output.content.contains("<skill_content"));
        assert!(output.content.contains("Do the thing."));
        assert!(output.content.contains("</skill_content>"));
    }

    #[tokio::test]
    async fn duplicate_activation_returns_already_active() {
        let catalog = test_catalog();
        let state = Arc::new(Mutex::new(SkillActivationState::default()));
        let tool = ActivateSkillTool::new(Arc::clone(&catalog), Arc::clone(&state));

        let input = serde_json::json!({ "name": "test-skill" });
        tool.execute_inner(input.clone()).await.unwrap();

        let output = tool.execute_inner(input).await.unwrap();
        assert!(output.content.contains("already active"));
    }

    #[tokio::test]
    async fn not_found_returns_error() {
        let catalog = test_catalog();
        let state = Arc::new(Mutex::new(SkillActivationState::default()));
        let tool = ActivateSkillTool::new(Arc::clone(&catalog), Arc::clone(&state));

        let input = serde_json::json!({ "name": "nonexistent" });
        let output = tool.execute_inner(input).await.unwrap();
        assert!(output.is_error);
    }

    #[test]
    fn metadata_has_dynamic_enum() {
        let catalog = test_catalog();
        let state = Arc::new(Mutex::new(SkillActivationState::default()));
        let tool = ActivateSkillTool::new(catalog, state);
        let meta = tool.metadata();

        assert_eq!(meta.name, "activate_skill");
        assert!(meta.context_modifying);
        assert!(meta.internal);

        let schema = &meta.input_schema;
        let enum_values = schema["properties"]["name"]["enum"].as_array().unwrap();
        assert!(enum_values.iter().any(|v| v == "test-skill"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && cargo test -p sober-skill -q`
Expected: FAIL

- [ ] **Step 3: Implement ActivateSkillTool**

```rust
//! The `activate_skill` tool — loads skill instructions into conversation context.

use std::sync::Arc;

use sober_core::types::tool::{BoxToolFuture, Tool, ToolMetadata, ToolOutput, ToolError};
use tokio::fs;

use crate::catalog::SkillCatalog;
use crate::frontmatter::parse_skill_frontmatter;
use crate::types::SkillActivationState;

/// Tool that loads a skill's instructions into the conversation context.
///
/// The LLM calls this when it determines a task matches an available skill's
/// description. The tool reads the SKILL.md body, enumerates supporting files,
/// and returns wrapped content.
///
/// Activation state is tracked internally via `Arc<Mutex<SkillActivationState>>`.
/// The agent creates one `ActivateSkillTool` per conversation turn (via
/// bootstrap), sharing the same activation state across turns within a
/// conversation.
pub struct ActivateSkillTool {
    catalog: Arc<SkillCatalog>,
    activation_state: Arc<std::sync::Mutex<SkillActivationState>>,
}

impl ActivateSkillTool {
    /// Creates a new tool backed by the given catalog and activation state.
    ///
    /// The same `activation_state` should be shared across all turns within
    /// a single conversation to prevent duplicate skill injection.
    pub fn new(
        catalog: Arc<SkillCatalog>,
        activation_state: Arc<std::sync::Mutex<SkillActivationState>>,
    ) -> Self {
        Self { catalog, activation_state }
    }

    /// Core execution logic.
    async fn execute_inner(
        &self,
        input: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing 'name' field".into()))?;

        // Check if skill exists
        let entry = self.catalog.get(name).ok_or_else(|| {
            ToolError::ExecutionFailed(format!("skill not found: {name}"))
        })?;

        // Check if already activated
        {
            let state = self.activation_state.lock()
                .map_err(|_| ToolError::Internal(
                    Box::new(std::io::Error::other("activation state lock poisoned"))
                ))?;
            if state.is_activated(name) {
                return Ok(ToolOutput {
                    content: format!("Skill '{name}' is already active in this conversation."),
                    is_error: false,
                });
            }
        }

        // Read SKILL.md and strip frontmatter
        let content = fs::read_to_string(&entry.path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to read SKILL.md: {e}")))?;

        let (_fm, body) = parse_skill_frontmatter(&content)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to parse SKILL.md: {e}")))?;

        // Enumerate supporting files
        let resources = enumerate_resources(&entry.base_dir).await;

        // Build wrapped response
        let mut result = format!("<skill_content name=\"{name}\">\n");

        // Include compatibility info if present
        if let Some(compat) = &entry.frontmatter.compatibility {
            result.push_str(&format!("Compatibility: {compat}\n\n"));
        }

        result.push_str(body);
        result.push_str(&format!(
            "\n\nSkill directory: {}\nRelative paths are relative to the skill directory.",
            entry.base_dir.display()
        ));

        if !resources.is_empty() {
            result.push_str("\n\n<skill_resources>\n");
            for resource in &resources {
                result.push_str(&format!("  <file>{resource}</file>\n"));
            }
            result.push_str("</skill_resources>");
        }

        result.push_str("\n</skill_content>");

        // Mark as activated
        {
            let mut state = self.activation_state.lock()
                .map_err(|_| ToolError::Internal(
                    Box::new(std::io::Error::other("activation state lock poisoned"))
                ))?;
            state.activate(name.to_string());
        }

        Ok(ToolOutput {
            content: result,
            is_error: false,
        })
    }
}

impl Tool for ActivateSkillTool {
    fn metadata(&self) -> ToolMetadata {
        let names = self.catalog.names();
        let enum_values: Vec<serde_json::Value> =
            names.iter().map(|n| serde_json::Value::String(n.to_string())).collect();

        ToolMetadata {
            name: "activate_skill".to_owned(),
            description: "Load specialized skill instructions into the conversation. \
                Call when a task matches an available skill's description."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "enum": enum_values,
                        "description": "The skill to activate"
                    }
                },
                "required": ["name"]
            }),
            context_modifying: true,
            internal: true,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

/// Enumerates files in scripts/, references/, and assets/ directories.
async fn enumerate_resources(base_dir: &std::path::Path) -> Vec<String> {
    let mut resources = Vec::new();

    for subdir in ["scripts", "references", "assets"] {
        let dir = base_dir.join(subdir);
        if !dir.is_dir() {
            continue;
        }
        let Ok(mut entries) = fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.path().is_file() {
                if let Some(name) = entry.file_name().to_str() {
                    resources.push(format!("{subdir}/{name}"));
                }
            }
        }
    }

    resources.sort();
    resources
}
```

- [ ] **Step 4: Update lib.rs**

Add `pub mod tool;` and re-export `pub use tool::ActivateSkillTool;`.

- [ ] **Step 5: Run tests**

Run: `cd backend && cargo test -p sober-skill -q`
Expected: All pass

- [ ] **Step 6: Run clippy**

Run: `cd backend && cargo clippy -p sober-skill -q -- -D warnings`
Expected: No warnings

- [ ] **Step 7: Commit**

```bash
git add backend/crates/sober-skill/
git commit -m "feat(skill): implement activate_skill tool with progressive disclosure"
```

---

## Chunk 2: Backend Integration

### Task 7: gRPC proto — add ListSkills RPC

**Files:**
- Modify: `backend/proto/sober/agent/v1/agent.proto`

- [ ] **Step 1: Add ListSkills messages and RPC**

Add to the `AgentService` definition:

```protobuf
rpc ListSkills(ListSkillsRequest) returns (ListSkillsResponse);
```

Add message definitions:

```protobuf
message ListSkillsRequest {
  string user_id = 1;
  optional string conversation_id = 2;
}

message SkillInfo {
  string name = 1;
  string description = 2;
}

message ListSkillsResponse {
  repeated SkillInfo skills = 1;
}
```

The agent resolves the user's home directory and workspace path internally
from the `user_id` (via user config) and `conversation_id` (via workspace
association). This keeps the gRPC contract clean — callers don't need to
know filesystem paths.

- [ ] **Step 2: Rebuild protos**

Run: `cd backend && cargo build -q -p sober-agent`
Expected: Success (proto compilation is part of build)

- [ ] **Step 3: Commit**

```bash
git add backend/proto/
git commit -m "feat(skill): add ListSkills RPC to agent proto"
```

---

### Task 8: Agent integration — SkillLoader + tool registration

**Files:**
- Modify: `backend/crates/sober-agent/Cargo.toml`
- Modify: `backend/crates/sober-agent/src/grpc.rs`
- Modify: `backend/crates/sober-agent/src/tools/bootstrap.rs`

- [ ] **Step 1: Add sober-skill dependency**

Add to `backend/crates/sober-agent/Cargo.toml` under `[dependencies]`:

```toml
sober-skill = { path = "../sober-skill" }
```

- [ ] **Step 2: Implement ListSkills RPC in grpc.rs**

Add `use sober_skill::SkillLoader;` and the `SkillLoader` to the
`AgentGrpcService` struct. Implement the `list_skills` method:

```rust
async fn list_skills(
    &self,
    request: Request<proto::ListSkillsRequest>,
) -> Result<Response<proto::ListSkillsResponse>, Status> {
    let req = request.into_inner();

    let user_id = req.user_id
        .parse::<uuid::Uuid>()
        .map(UserId::from_uuid)
        .map_err(|_| Status::invalid_argument("invalid user_id"))?;

    // Resolve filesystem paths from user/conversation context.
    // User home: look up from config or use process HOME.
    // Workspace: resolve from conversation's workspace association.
    let user_home = self.resolve_user_home(&user_id);
    let workspace_path = match req.conversation_id {
        Some(conv_id) => {
            let conv_id = conv_id.parse::<uuid::Uuid>()
                .map(ConversationId::from_uuid)
                .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;
            self.resolve_workspace_path(&conv_id).await
        }
        None => PathBuf::new(),
    };

    let catalog = self.skill_loader.load(&user_home, &workspace_path)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let skills = catalog.names().iter().map(|name| {
        let entry = catalog.get(name).unwrap();
        proto::SkillInfo {
            name: entry.frontmatter.name.clone(),
            description: entry.frontmatter.description.clone(),
        }
    }).collect();

    Ok(Response::new(proto::ListSkillsResponse { skills }))
}
```

The `resolve_user_home` and `resolve_workspace_path` helper methods
encapsulate path resolution logic. For single-user deployments,
`resolve_user_home` can simply return `$HOME`. Multi-user support
can be added later without changing the gRPC contract.

- [ ] **Step 3: Add SkillLoader to AgentGrpcService constructor**

The `AgentGrpcService` struct gains a `skill_loader: Arc<SkillLoader>` field.
Pass it through from the agent's main setup.

- [ ] **Step 4: Register ActivateSkillTool in bootstrap.rs**

In `build_static_tools()`, add:

```rust
// Skill tool is registered per-turn (needs current catalog)
// Add to the per-turn build method, not static tools
```

In the per-turn `build()` method, load the catalog and create the tool:

```rust
let catalog = self.skill_loader.load(&user_home, &workspace_path).await?;
if !catalog.is_empty() {
    tools.push(Arc::new(ActivateSkillTool::new(catalog)));
}
```

- [ ] **Step 5: Build and test**

Run: `cd backend && cargo build -q -p sober-agent`
Run: `cd backend && cargo test -p sober-agent -q`
Expected: Both succeed

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-agent/
git commit -m "feat(skill): integrate SkillLoader and activate_skill tool into agent"
```

---

### Task 9: Mind integration — skill catalog in system prompt

**Files:**
- Modify: `backend/crates/sober-mind/Cargo.toml`
- Modify: `backend/crates/sober-mind/src/assembly.rs`

- [ ] **Step 1: Add sober-skill dependency**

Add to `backend/crates/sober-mind/Cargo.toml`:

```toml
sober-skill = { path = "../sober-skill" }
```

- [ ] **Step 2: Modify build_system_prompt to accept skill catalog**

Add `skill_catalog_xml: &str` parameter to `build_system_prompt`. Insert
the skill catalog between instruction bodies and tool definitions:

```rust
// 5. Concatenate all instruction bodies
// ... existing code ...

// 5.5. Inject skill catalog (after instructions, before tools)
if !skill_catalog_xml.is_empty() {
    prompt.push_str("\n\n");
    prompt.push_str(skill_catalog_xml);
}

// 6. Append tool definitions
// ... existing code ...
```

- [ ] **Step 3: Update callers of build_system_prompt**

The public `assemble()` method must pass the skill catalog XML. Add
`skill_catalog_xml: &str` parameter to `assemble()` as well.

- [ ] **Step 4: Build and test**

Run: `cd backend && cargo build -q -p sober-mind -p sober-agent`
Run: `cd backend && cargo test -p sober-mind -q`
Expected: Both succeed

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-mind/ backend/crates/sober-agent/
git commit -m "feat(mind): inject skill catalog into system prompt assembly"
```

---

### Task 10: API endpoint — GET /api/v1/skills

**Files:**
- Create: `backend/crates/sober-api/src/routes/skills.rs`
- Modify: `backend/crates/sober-api/src/routes/mod.rs`

- [ ] **Step 1: Create skills route handler**

```rust
//! Skill listing route — proxies to agent gRPC service.

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::routing::get;
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::ApiResponse;

use crate::proto;
use crate::state::AppState;

/// Returns the skills routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/skills", get(list_skills))
}

/// `GET /api/v1/skills?conversation_id=<uuid>` — list available skills.
///
/// Proxies to the agent's `ListSkills` gRPC RPC. Returns skill names and
/// descriptions for frontend slash command registration.
///
/// The optional `conversation_id` query parameter scopes skill discovery
/// to the conversation's workspace. Without it, only user-level skills
/// are returned.
async fn list_skills(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    axum::extract::Query(params): axum::extract::Query<ListSkillsParams>,
) -> Result<ApiResponse<Vec<serde_json::Value>>, AppError> {
    let mut client = state.agent_client.clone();

    let response = client
        .list_skills(proto::ListSkillsRequest {
            user_id: auth_user.user_id.to_string(),
            conversation_id: params.conversation_id,
        })
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let skills: Vec<serde_json::Value> = response
        .into_inner()
        .skills
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "description": s.description,
            })
        })
        .collect();

    Ok(ApiResponse::new(skills))
}

#[derive(serde::Deserialize)]
struct ListSkillsParams {
    conversation_id: Option<String>,
}
```

- [ ] **Step 2: Register in routes/mod.rs**

Add `pub mod skills;` to the module declarations.
Add `.merge(skills::routes())` to `build_router`.

- [ ] **Step 3: Build**

Run: `cd backend && cargo build -q -p sober-api`
Expected: Success

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-api/src/routes/
git commit -m "feat(api): add GET /api/v1/skills endpoint proxying to agent"
```

---

## Chunk 3: Frontend Integration

### Task 11: Skills service and types

**Files:**
- Create: `frontend/src/lib/services/skills.ts`
- Modify: `frontend/src/lib/types/index.ts`

- [ ] **Step 1: Add SkillInfo type**

Add to `frontend/src/lib/types/index.ts`:

```typescript
export interface SkillInfo {
    name: string;
    description: string;
}
```

- [ ] **Step 2: Create skills service**

```typescript
import { api } from '$lib/utils/api';
import type { SkillInfo } from '$lib/types';

export const skillsService = {
    list: () => api<SkillInfo[]>('/skills'),
};
```

- [ ] **Step 3: Verify TypeScript**

Run: `cd frontend && pnpm check`
Expected: No type errors

- [ ] **Step 4: Commit**

```bash
git add frontend/src/lib/services/skills.ts frontend/src/lib/types/index.ts
git commit -m "feat(frontend): add skills API service and types"
```

---

### Task 12: Slash command autocomplete integration

**Files:**
- Modify: Chat input component (find the component that handles `/` commands)

- [ ] **Step 1: Identify the chat input component**

Search for the existing slash command handling:
`grep -r "slash\|autocomplete\|command" frontend/src/lib/components/`

- [ ] **Step 2: Fetch skills on app load**

In the appropriate layout or store, fetch skills and make them available:

```typescript
import { skillsService } from '$lib/services/skills';
import type { SkillInfo } from '$lib/types';

let skills = $state<SkillInfo[]>([]);

async function loadSkills() {
    try {
        skills = await skillsService.list();
    } catch {
        // Skills are optional — don't block on failure
        skills = [];
    }
}
```

- [ ] **Step 3: Merge skill commands into autocomplete**

Add skill names to the `/` command autocomplete menu with a distinct
indicator (e.g., a different icon or label like "[skill]").

- [ ] **Step 4: Handle skill command invocation**

When a skill slash command is selected, prepend `/skill-name ` to the
message. The backend intercepts and activates.

- [ ] **Step 5: Test manually**

Run: `cd frontend && pnpm dev`
Test: Type `/` in chat, verify skill commands appear in autocomplete.

- [ ] **Step 6: Commit**

```bash
git add frontend/
git commit -m "feat(frontend): integrate skill slash commands into chat autocomplete"
```

---

## Chunk 4: Verification and Cleanup

### Task 13: Full build and test verification

- [ ] **Step 1: Full workspace build**

Run: `cd backend && cargo build -q`
Expected: Success

- [ ] **Step 2: Clippy across workspace**

Run: `cd backend && cargo clippy -q -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Run all backend tests**

Run: `cd backend && cargo test --workspace -q`
Expected: All pass

- [ ] **Step 4: Frontend checks**

Run: `cd frontend && pnpm check && pnpm test --silent`
Expected: All pass

- [ ] **Step 5: Commit any final fixes**

---

### Task 14: Version bumps and plan lifecycle

**Files:**
- Modify: `backend/crates/sober-mind/Cargo.toml` — version `0.5.0` → `0.6.0`
- Modify: `backend/crates/sober-agent/Cargo.toml` — version `0.11.0` → `0.12.0`
- Modify: `backend/crates/sober-api/Cargo.toml` — minor bump
- Move: `docs/plans/active/033-skill-support/` → `docs/plans/done/033-skill-support/`

- [ ] **Step 1: Bump versions**

Update version fields in the affected `Cargo.toml` files.

- [ ] **Step 2: Build to verify**

Run: `cd backend && cargo build -q`

- [ ] **Step 3: Move plan to done**

```bash
git mv docs/plans/active/033-skill-support docs/plans/done/033-skill-support
```

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "#033: Skill support — format, loader, and activation"
```
