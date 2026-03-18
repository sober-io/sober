//! Agent Skills spec-compliant skill discovery, loading, and activation.

pub mod catalog;
pub mod error;
pub mod frontmatter;
pub mod loader;
pub mod tool;
pub mod types;

pub use catalog::SkillCatalog;
pub use error::SkillError;
pub use frontmatter::{SkillFrontmatter, parse_skill_frontmatter, validate_skill_name};
pub use loader::SkillLoader;
pub use tool::ActivateSkillTool;
pub use types::{SkillActivationState, SkillEntry, SkillSource};
