//! Frontmatter types and YAML parsing for instruction files.
//!
//! Every `.md` file in the prompt directory has YAML frontmatter with metadata
//! that controls assembly order and visibility filtering.

use std::fmt;

use serde::Deserialize;

/// Instruction category — determines assembly order.
///
/// Categories are sorted in the order they appear in the enum:
/// `Personality` → `Guardrail` → `Behavior` → `Operation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum InstructionCategory {
    Personality,
    Guardrail,
    Behavior,
    Operation,
}

impl fmt::Display for InstructionCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Personality => write!(f, "personality"),
            Self::Guardrail => write!(f, "guardrail"),
            Self::Behavior => write!(f, "behavior"),
            Self::Operation => write!(f, "operation"),
        }
    }
}

/// Visibility level — controls which triggers can see an instruction file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    /// Visible to all triggers (Human, Replica, Admin, Scheduler).
    #[default]
    Public,
    /// Visible only to Admin and Scheduler triggers.
    Internal,
}

impl fmt::Display for Visibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Public => write!(f, "public"),
            Self::Internal => write!(f, "internal"),
        }
    }
}

/// Parsed YAML frontmatter from an instruction file.
#[derive(Debug, Clone, Deserialize)]
pub struct InstructionFrontmatter {
    /// Which category this instruction belongs to.
    pub category: InstructionCategory,

    /// Who can see this instruction. Defaults to `Public`.
    #[serde(default)]
    pub visibility: Visibility,

    /// Sort priority within a category (lower = earlier). Defaults to 50.
    #[serde(default = "default_priority")]
    pub priority: u32,

    /// Optional: name of the base file this extends (for user/workspace layers).
    pub extends: Option<String>,
}

fn default_priority() -> u32 {
    50
}

/// Parses YAML frontmatter from a markdown file's raw content.
///
/// Expects the file to start with `---\n`, contain YAML, and end with `---\n`.
/// Returns the parsed frontmatter and the remaining markdown body.
pub fn parse_frontmatter(
    content: &str,
) -> Result<(InstructionFrontmatter, &str), FrontmatterError> {
    let content = content.trim_start();

    if !content.starts_with("---") {
        return Err(FrontmatterError::MissingDelimiter);
    }

    // Skip the opening "---" and the newline after it.
    let after_open = &content[3..];
    let after_open = after_open.strip_prefix('\n').unwrap_or(after_open);

    let Some(close_idx) = after_open.find("\n---") else {
        return Err(FrontmatterError::MissingDelimiter);
    };

    let yaml_str = &after_open[..close_idx];
    let body_start = close_idx + 4; // skip "\n---"
    let body = if body_start < after_open.len() {
        // Skip the newline after the closing "---"
        let rest = &after_open[body_start..];
        rest.strip_prefix('\n').unwrap_or(rest)
    } else {
        ""
    };

    let frontmatter: InstructionFrontmatter =
        serde_yml::from_str(yaml_str).map_err(|e| FrontmatterError::ParseFailed(e.to_string()))?;

    Ok((frontmatter, body))
}

/// Errors from frontmatter parsing.
#[derive(Debug, thiserror::Error)]
pub enum FrontmatterError {
    /// File is missing the `---` frontmatter delimiters.
    #[error("missing frontmatter delimiters (expected ---\\n...\\n---)")]
    MissingDelimiter,

    /// YAML parsing failed.
    #[error("frontmatter parse failed: {0}")]
    ParseFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_frontmatter() {
        let content = "---\ncategory: personality\nvisibility: public\npriority: 10\n---\n# Soul\nContent here.";
        let (fm, body) = parse_frontmatter(content).unwrap();
        assert_eq!(fm.category, InstructionCategory::Personality);
        assert_eq!(fm.visibility, Visibility::Public);
        assert_eq!(fm.priority, 10);
        assert!(fm.extends.is_none());
        assert_eq!(body, "# Soul\nContent here.");
    }

    #[test]
    fn defaults_visibility_and_priority() {
        let content = "---\ncategory: operation\n---\n# Tools";
        let (fm, body) = parse_frontmatter(content).unwrap();
        assert_eq!(fm.category, InstructionCategory::Operation);
        assert_eq!(fm.visibility, Visibility::Public);
        assert_eq!(fm.priority, 50);
        assert_eq!(body, "# Tools");
    }

    #[test]
    fn parses_extends_field() {
        let content = "---\ncategory: behavior\nextends: memory.md\n---\nExtra memory rules.";
        let (fm, body) = parse_frontmatter(content).unwrap();
        assert_eq!(fm.extends.as_deref(), Some("memory.md"));
        assert_eq!(body, "Extra memory rules.");
    }

    #[test]
    fn internal_visibility() {
        let content =
            "---\ncategory: behavior\nvisibility: internal\npriority: 20\n---\nReasoning.";
        let (fm, _) = parse_frontmatter(content).unwrap();
        assert_eq!(fm.visibility, Visibility::Internal);
    }

    #[test]
    fn missing_delimiters_errors() {
        assert!(parse_frontmatter("# No frontmatter").is_err());
        assert!(parse_frontmatter("---\ncategory: personality\nNo closing").is_err());
    }

    #[test]
    fn invalid_yaml_errors() {
        let content = "---\ncategory: nonexistent\n---\nBody.";
        assert!(parse_frontmatter(content).is_err());
    }

    #[test]
    fn category_ordering() {
        assert!(InstructionCategory::Personality < InstructionCategory::Guardrail);
        assert!(InstructionCategory::Guardrail < InstructionCategory::Behavior);
        assert!(InstructionCategory::Behavior < InstructionCategory::Operation);
    }

    #[test]
    fn empty_body() {
        let content = "---\ncategory: guardrail\n---\n";
        let (fm, body) = parse_frontmatter(content).unwrap();
        assert_eq!(fm.category, InstructionCategory::Guardrail);
        assert_eq!(body, "");
    }
}
