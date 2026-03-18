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
    if name
        .chars()
        .any(|c| !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-')
    {
        warnings
            .push("name must contain only lowercase alphanumeric characters and hyphens".into());
    }

    warnings
}

/// Parses a SKILL.md file into frontmatter and body.
///
/// Expects YAML frontmatter between `---` delimiters followed by markdown.
/// Returns `(frontmatter, body)` where body is trimmed.
pub fn parse_skill_frontmatter(content: &str) -> Result<(SkillFrontmatter, &str), SkillError> {
    let content = content.trim_start();

    if !content.starts_with("---") {
        return Err(SkillError::FrontmatterParseFailed {
            path: String::new(),
            reason: "missing opening --- delimiter".into(),
        });
    }

    let after_open = &content[3..].trim_start_matches(['\r', '\n']);
    let closing = after_open
        .find("\n---")
        .ok_or_else(|| SkillError::FrontmatterParseFailed {
            path: String::new(),
            reason: "missing closing --- delimiter".into(),
        })?;

    let yaml_str = &after_open[..closing];
    let body_start = closing + 4; // skip \n---
    let body = after_open[body_start..].trim();

    let fm: SkillFrontmatter =
        serde_yml::from_str(yaml_str).map_err(|e| SkillError::FrontmatterParseFailed {
            path: String::new(),
            reason: e.to_string(),
        })?;

    if fm.description.trim().is_empty() {
        return Err(SkillError::FrontmatterParseFailed {
            path: String::new(),
            reason: "description must not be empty".into(),
        });
    }

    Ok((fm, body))
}

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
        assert_eq!(
            fm.allowed_tools,
            Some(vec!["Bash(git:*)".into(), "Read".into()])
        );
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
        assert!(validate_skill_name("my-skill").is_empty());
        assert!(validate_skill_name("a").is_empty());
        assert!(validate_skill_name("code-review").is_empty());

        assert!(!validate_skill_name("My-Skill").is_empty());
        assert!(!validate_skill_name("-leading").is_empty());
        assert!(!validate_skill_name("trailing-").is_empty());
        assert!(!validate_skill_name("double--hyphen").is_empty());
    }
}
