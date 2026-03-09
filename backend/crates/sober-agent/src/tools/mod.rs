//! Built-in agent tools and the tool registry.
//!
//! This module provides the [`ToolRegistry`](registry::ToolRegistry) for
//! discovering and invoking tools, along with built-in implementations:
//!
//! - [`WebSearchTool`](web_search::WebSearchTool) — web search via SearXNG
//! - [`FetchUrlTool`](fetch_url::FetchUrlTool) — fetch and extract URL content

pub mod fetch_url;
pub mod registry;
pub mod web_search;

pub use fetch_url::FetchUrlTool;
pub use registry::ToolRegistry;
pub use web_search::WebSearchTool;
