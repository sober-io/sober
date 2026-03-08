//! Importance scoring for memory chunks.

mod importance;

pub use importance::{boost, decay, should_prune};
