//! Agent identity (SOUL.md), prompt assembly, access masks, trait evolution,
//! and injection detection for the Sober system.
//!
//! `sober-mind` is the agent's cognitive context layer. It owns:
//!
//! - **SOUL.md resolution** — layered identity from base, user, and workspace files
//! - **Injection detection** — heuristic classifier that runs before prompt assembly
//! - **Prompt assembly** — dynamic system prompt composition from soul + context + tools
//! - **Access masks** — filters prompt content based on trigger source
//! - **Soul layers** — per-user/group adaptations stored in BCF memory
//! - **Trait evolution** — autonomous refinement of soul layers (v1 stub)

pub mod access;
pub mod assembly;
pub mod error;
pub mod evolution;
pub mod injection;
pub mod layers;
pub mod soul;

pub use access::apply_access_mask;
pub use assembly::Mind;
pub use error::MindError;
pub use evolution::{EvolutionAuditEntry, EvolutionDecision, TraitCandidate};
pub use injection::{InjectionVerdict, classify_input};
pub use layers::SoulLayer;
pub use soul::SoulResolver;
