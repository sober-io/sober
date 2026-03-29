//! Evolution execution engine.
//!
//! Handles executing approved evolution events and reverting active ones.
//! Each evolution type (Plugin, Skill, Instruction, Automation) has its own
//! execution and revert logic. Instruction evolutions are fully implemented;
//! other types are stub implementations pending future infrastructure.

pub mod executor;
pub mod revert;

pub use executor::execute_evolution;
pub use revert::revert_evolution;
