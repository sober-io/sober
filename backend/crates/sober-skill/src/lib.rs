//! Agent Skills spec-compliant skill discovery, loading, and activation.

pub mod error;
pub mod frontmatter;
pub mod types;

pub use error::SkillError;
pub use frontmatter::{SkillFrontmatter, parse_skill_frontmatter, validate_skill_name};
pub use types::{SkillActivationState, SkillEntry, SkillSource};
