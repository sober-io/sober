//! Agent identity, prompt assembly, visibility filtering, trait evolution,
//! and injection detection for the Sober system.
//!
//! `sober-mind` is the agent's cognitive context layer. It owns:
//!
//! - **Structured instructions** — split `.md` files with YAML frontmatter
//! - **soul.md resolution** — layered identity from base, user, and workspace files
//! - **Injection detection** — heuristic classifier that runs before prompt assembly
//! - **Prompt assembly** — dynamic system prompt composition from instructions + tools
//! - **Visibility filtering** — frontmatter-driven access control per trigger kind
//! - **Soul layers** — per-user/group adaptations stored in BCF memory
//! - **Trait evolution** — autonomous refinement of soul layers (v1 stub)

pub mod access;
pub mod assembly;
pub mod error;
pub mod evolution;
pub mod frontmatter;
pub mod injection;
pub mod instructions;
pub mod layers;
pub mod references;
pub mod soul;

pub use access::is_visible;
pub use assembly::Mind;
pub use error::MindError;
pub use evolution::{EvolutionAuditEntry, EvolutionDecision, TraitCandidate, is_guardrail_file};
pub use frontmatter::{InstructionCategory, InstructionFrontmatter, Visibility};
pub use injection::{InjectionVerdict, classify_input};
pub use instructions::{InstructionFile, InstructionLoader};
pub use layers::SoulLayer;
pub use soul::SoulResolver;
