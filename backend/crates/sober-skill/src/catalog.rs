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
    /// Returns an empty string if there are no skills.
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
        skills.insert(
            "code-review".into(),
            test_entry("code-review", "Reviews code."),
        );
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
