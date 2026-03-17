//! MCP client library for tool interop.
//!
//! `sober-mcp` implements the [Model Context Protocol](https://spec.modelcontextprotocol.io/)
//! client side, connecting to local MCP servers over stdio (JSON-RPC 2.0).
//! MCP server processes are spawned via `sober-sandbox` for process-level
//! isolation.
//!
//! # Architecture
//!
//! - [`McpClient`] manages a single MCP server connection (stdio transport).
//! - [`McpPool`] manages multiple server connections per user with crash recovery.
//! - [`McpToolAdapter`] and [`McpResourceAdapter`] wrap MCP capabilities for
//!   use by `sober-agent` via the [`Tool`](sober_core::types::Tool) trait.

pub mod adapter;
pub mod client;
pub mod config;
pub mod credentials;
pub mod error;
pub mod pool;
pub mod types;

mod jsonrpc;

pub use adapter::{McpResourceAdapter, McpToolAdapter};
pub use client::McpClient;
pub use config::McpConfig;
pub use error::McpError;
pub use pool::McpPool;
pub use types::{McpResourceInfo, McpToolInfo, ResourceContent, ServerCapabilities, ServerInfo};
