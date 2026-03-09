//! Multi-provider LLM abstraction with OpenAI-compatible HTTP and ACP
//! transports.
//!
//! This crate is the sole interface to LLM providers — no other crate should
//! access external LLM APIs directly.
//!
//! # Engines
//!
//! - [`OpenAiCompatibleEngine`] — HTTP client for any provider that implements
//!   the OpenAI Chat Completions API format (OpenRouter, OpenAI, Ollama, etc.).
//! - [`AcpEngine`] — Sends prompts through local ACP-compatible agents
//!   (Claude Code, Kimi Code, Goose) via JSON-RPC/stdio.
//!
//! # Modules
//!
//! - [`engine`] — [`LlmEngine`] trait for engine abstraction.
//! - [`client`] — [`OpenAiCompatibleEngine`].
//! - [`acp`] — [`AcpEngine`] for Agent Client Protocol.
//! - [`types`] — Request/response types for LLM operations.
//! - [`error`] — [`LlmError`] with `AppError` integration.
//! - [`streaming`] — SSE parser for streaming responses.
//! - [`jsonrpc`] — JSON-RPC 2.0 transport for ACP subprocess communication.

pub mod acp;
pub mod client;
pub mod engine;
pub mod error;
pub mod jsonrpc;
pub mod streaming;
pub mod types;

pub use acp::{AcpConfig, AcpEngine};
pub use client::OpenAiCompatibleEngine;
pub use engine::LlmEngine;
pub use error::LlmError;
pub use types::{CompletionRequest, CompletionResponse, EngineCapabilities, Message};
