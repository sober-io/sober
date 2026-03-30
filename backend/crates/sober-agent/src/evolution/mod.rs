//! Evolution execution engine.
//!
//! Handles executing approved evolution events and reverting active ones.
//! Each evolution type (Plugin, Skill, Instruction, Automation) has its own
//! execution and revert logic.

pub mod executor;
pub mod revert;

pub use executor::{EvolutionContext, execute_evolution};
pub use revert::revert_evolution;
