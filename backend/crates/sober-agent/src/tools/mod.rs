//! Built-in agent tools and the tool registry.
//!
//! This module provides the [`ToolRegistry`](registry::ToolRegistry) for
//! discovering and invoking tools, along with built-in implementations:
//!
//! - [`WebSearchTool`](web_search::WebSearchTool) — web search via SearXNG
//! - [`FetchUrlTool`](fetch_url::FetchUrlTool) — fetch and extract URL content
//! - [`ShellTool`](shell::ShellTool) — sandboxed shell command execution
//! - [`RecallTool`](memory::RecallTool) — active memory search
//! - [`RememberTool`](memory::RememberTool) — explicit memory storage

pub mod fetch_url;
pub mod memory;
pub mod registry;
pub mod scheduler;
pub mod shell;
pub mod web_search;

pub use fetch_url::FetchUrlTool;
pub use memory::{RecallTool, RememberTool};
pub use registry::ToolRegistry;
pub use scheduler::SchedulerTools;
pub use shell::{SharedPermissionMode, ShellTool};
pub use web_search::WebSearchTool;
