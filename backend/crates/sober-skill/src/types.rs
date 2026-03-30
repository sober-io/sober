//! Core types for skill discovery and activation.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::frontmatter::SkillFrontmatter;

/// Where a skill was discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    /// System-level: `workspace_root/.sober/skills/` (configurable at startup).
    System,
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
