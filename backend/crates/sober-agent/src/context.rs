//! Shared agent dependencies passed through the pipeline.
//!
//! [`AgentContext`] groups the `Arc`-cloned services that [`ConversationActor`],
//! [`TurnParams`], and dispatch functions all need, reducing argument counts and
//! making it easy to add new dependencies without touching every call site.

use std::sync::Arc;

use sober_core::config::{LlmConfig, MemoryConfig};
use sober_core::types::AgentRepos;
use sober_crypto::envelope::Mek;
use sober_llm::LlmEngine;
use sober_memory::{ContextLoader, MemoryStore};
use sober_mind::assembly::Mind;

use crate::agent::AgentConfig;
use crate::broadcast::ConversationUpdateSender;
use crate::confirm::ConfirmationRegistrar;
use crate::tools::ToolBootstrap;

/// Shared dependencies passed through the agent pipeline.
///
/// Groups the `Arc`-cloned services that [`ConversationActor`](crate::conversation::ConversationActor),
/// [`TurnParams`](crate::turn::TurnParams), and dispatch functions all need,
/// reducing argument counts.
pub struct AgentContext<R: AgentRepos> {
    /// LLM engine for completions and embeddings.
    pub llm: Arc<dyn LlmEngine>,
    /// Prompt assembly engine.
    pub mind: Arc<Mind>,
    /// Vector memory store for knowledge retrieval.
    pub memory: Arc<MemoryStore>,
    /// Context loader for conversation history + memory retrieval.
    pub context_loader: Arc<ContextLoader<R::Msg>>,
    /// Repository bundle (messages, conversations, tool executions, etc.).
    pub repos: Arc<R>,
    /// Agent behaviour configuration.
    pub config: AgentConfig,
    /// Memory subsystem configuration.
    pub memory_config: MemoryConfig,
    /// Registrar for interactive confirmation of dangerous tool calls.
    pub registrar: Option<ConfirmationRegistrar>,
    /// Broadcast sender for conversation update events.
    pub broadcast_tx: ConversationUpdateSender,
    /// Master encryption key for resolving user-stored LLM keys.
    pub mek: Option<Arc<Mek>>,
    /// LLM config for constructing dynamic engines from resolved keys.
    pub llm_config: Option<LlmConfig>,
    /// Centralized tool construction — builds a complete [`ToolRegistry`]
    /// per conversation turn with the correct workspace paths and scoped tools.
    pub tool_bootstrap: Arc<ToolBootstrap<R>>,
    /// Pre-built static tools (web_search, fetch_url, scheduler, recall, remember).
    pub static_tools: Vec<Arc<dyn sober_core::types::tool::Tool>>,
}
