//! Core agent logic and per-conversation actor routing.
//!
//! The [`Agent`] struct owns an [`ActorRegistry`] that maps active conversations
//! to their [`ConversationActor`] tasks. When a message arrives for a
//! conversation, the agent either routes it to an existing actor or spawns a new
//! one. The actual agentic loop lives in [`turn::run_turn`]; the actor manages
//! its lifecycle (idle timeout, crash recovery, shutdown).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use sober_core::config::{LlmConfig, MemoryConfig};
use sober_core::types::AgentRepos;
use sober_core::types::ContentBlock;
use sober_core::types::access::TriggerKind;
use sober_core::types::ids::{ConversationId, UserId, WorkspaceId};
use sober_core::types::repo::{ConversationRepo, WorkspaceRepo};
use sober_crypto::envelope::Mek;
use sober_llm::{LlmEngine, OpenAiCompatibleEngine};
use sober_memory::{ContextLoader, LoadedContext, MemoryStore};
use sober_mind::assembly::Mind;
use sober_mind::injection::InjectionVerdict;

use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::broadcast::ConversationUpdateSender;
use crate::confirm::ConfirmationRegistrar;
use crate::context::AgentContext;
use crate::conversation::{ConversationActor, InboxMessage};
use crate::error::AgentError;
use crate::stream::{AgentEvent, AgentResponseStream};
use crate::tools::ToolBootstrap;

/// Buffer size for the per-message event channel.
const EVENT_CHANNEL_BUFFER: usize = 64;

/// Buffer size for an actor's inbox channel.
const ACTOR_INBOX_BUFFER: usize = 16;

// ---------------------------------------------------------------------------
// ActorRegistry — maps active conversations to their actor inbox senders
// ---------------------------------------------------------------------------

/// Handle to a running [`ConversationActor`]'s inbox channel.
struct ActorHandle {
    inbox_tx: mpsc::Sender<InboxMessage>,
}

/// Registry of active per-conversation actors.
///
/// Thread-safe (`DashMap`) so that multiple gRPC handler tasks can route
/// messages concurrently. Wrapped in `Arc` and shared between the [`Agent`]
/// and spawned actor tasks (which remove themselves on exit).
#[derive(Clone)]
pub struct ActorRegistry {
    actors: Arc<DashMap<ConversationId, ActorHandle>>,
}

impl ActorRegistry {
    /// Creates an empty registry.
    fn new() -> Self {
        Self {
            actors: Arc::new(DashMap::new()),
        }
    }

    /// Returns a clone of the inbox sender for an existing actor, or `None`.
    fn get_sender(&self, id: &ConversationId) -> Option<mpsc::Sender<InboxMessage>> {
        self.actors.get(id).map(|entry| entry.inbox_tx.clone())
    }

    /// Inserts a new actor handle.
    fn insert(&self, id: ConversationId, handle: ActorHandle) {
        self.actors.insert(id, handle);
    }

    /// Removes an actor from the registry (called when the actor task exits).
    fn remove(&self, id: &ConversationId) {
        self.actors.remove(id);
    }

    /// Sends [`InboxMessage::Shutdown`] to every active actor and waits
    /// briefly for them to drain.
    async fn shutdown_all(&self) {
        let count = self.actors.len();
        if count == 0 {
            return;
        }
        info!(count, "shutting down conversation actors");
        for entry in self.actors.iter() {
            let _ = entry.value().inbox_tx.send(InboxMessage::Shutdown).await;
        }
        // Give actors a moment to finish their current message.
        tokio::time::sleep(Duration::from_secs(2)).await;
        self.actors.clear();
    }
}

// ---------------------------------------------------------------------------
// AgentConfig
// ---------------------------------------------------------------------------

/// Configuration for agent behaviour.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Maximum number of tool-call iterations before aborting.
    pub max_tool_iterations: u32,
    /// Token budget for context loading.
    pub context_token_budget: usize,
    /// Number of recent messages to include from the conversation.
    pub conversation_history_limit: i64,
    /// Maximum memory hits per scope.
    pub hits_per_scope: u64,
    /// Default model identifier for completions.
    pub model: String,
    /// Default model identifier for embeddings.
    pub embedding_model: String,
    /// Maximum tokens to generate per completion.
    pub max_tokens: u32,
    /// Root directory for workspaces.
    pub workspace_root: PathBuf,
    /// Whether the configured model supports vision (image) inputs.
    pub vision: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_tool_iterations: 25,
            context_token_budget: 128_000,
            conversation_history_limit: 200,
            hits_per_scope: 5,
            model: "anthropic/claude-sonnet-4".to_owned(),
            embedding_model: "text-embedding-3-small".to_owned(),
            max_tokens: 4096,
            workspace_root: PathBuf::from("/var/lib/sober/workspaces"),
            vision: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Agent — the top-level orchestrator
// ---------------------------------------------------------------------------

/// The core agent that routes messages to per-conversation actors.
///
/// Generic over [`AgentRepos`] so it can be tested without a real database.
pub struct Agent<R: AgentRepos> {
    llm: Arc<dyn LlmEngine>,
    mind: Arc<Mind>,
    memory: Arc<MemoryStore>,
    context_loader: Arc<ContextLoader<R::Msg>>,
    repos: Arc<R>,
    config: AgentConfig,
    memory_config: MemoryConfig,
    /// Registrar for interactive confirmation of dangerous tool calls.
    registrar: Option<ConfirmationRegistrar>,
    /// Broadcast sender for conversation update events.
    broadcast_tx: ConversationUpdateSender,
    /// Master encryption key for resolving user-stored LLM keys.
    mek: Option<Arc<Mek>>,
    /// LLM config for constructing dynamic engines from resolved keys.
    llm_config: Option<LlmConfig>,
    /// Centralized tool construction — builds a complete [`ToolRegistry`]
    /// per conversation turn with the correct workspace paths and scoped tools.
    tool_bootstrap: Arc<ToolBootstrap<R>>,
    /// Pre-built static tools (web_search, fetch_url, scheduler, recall,
    /// remember) that are identical across conversations. Built once at
    /// construction and reused every turn.
    static_tools: Vec<Arc<dyn sober_core::types::tool::Tool>>,
    /// Per-conversation skill activation tracking. Skills activated in one turn
    /// remain active in subsequent turns of the same conversation.
    skill_activations: std::sync::RwLock<
        HashMap<ConversationId, Arc<std::sync::Mutex<sober_skill::SkillActivationState>>>,
    >,
    /// Registry of active per-conversation actors.
    registry: ActorRegistry,
}

impl<R: AgentRepos> Agent<R> {
    /// Creates a new agent with all required dependencies.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        llm: Arc<dyn LlmEngine>,
        mind: Arc<Mind>,
        memory: Arc<MemoryStore>,
        context_loader: Arc<ContextLoader<R::Msg>>,
        repos: Arc<R>,
        config: AgentConfig,
        memory_config: MemoryConfig,
        registrar: Option<ConfirmationRegistrar>,
        broadcast_tx: ConversationUpdateSender,
        mek: Option<Arc<Mek>>,
        llm_config: Option<LlmConfig>,
        tool_bootstrap: Arc<ToolBootstrap<R>>,
    ) -> Self {
        let static_tools = tool_bootstrap.build_static_tools();
        Self {
            llm,
            mind,
            memory,
            context_loader,
            repos,
            config,
            memory_config,
            registrar,
            broadcast_tx,
            mek,
            llm_config,
            tool_bootstrap,
            static_tools,
            skill_activations: std::sync::RwLock::new(HashMap::new()),
            registry: ActorRegistry::new(),
        }
    }

    /// Returns a reference to the agent's [`Mind`] instance.
    pub fn mind(&self) -> &Arc<Mind> {
        &self.mind
    }

    /// Returns a reference to the agent's repository bundle.
    pub fn repos(&self) -> &R {
        &self.repos
    }

    /// Returns a reference to the LLM engine.
    pub fn llm(&self) -> &Arc<dyn LlmEngine> {
        &self.llm
    }

    /// Returns the LLM config (model settings).
    pub fn llm_config(&self) -> &Option<LlmConfig> {
        &self.llm_config
    }

    /// Returns the tool bootstrap for enumerating built-in tools.
    pub fn tool_bootstrap(&self) -> &ToolBootstrap<R> {
        &self.tool_bootstrap
    }

    /// Resolves the workspace directory path for a conversation.
    ///
    /// Returns `None` if the conversation has no workspace or the workspace
    /// doesn't exist in the database.
    pub async fn resolve_workspace_dir(&self, conversation_id: ConversationId) -> Option<PathBuf> {
        let conversation = self
            .repos
            .conversations()
            .get_by_id(conversation_id)
            .await
            .ok()?;
        let ws_id = conversation.workspace_id?;
        let ws = self.repos.workspaces().get_by_id(ws_id).await.ok()?;
        Some(PathBuf::from(&ws.root_path))
    }

    /// Returns the skill activation state for a conversation, creating one if needed.
    fn skill_activation_state(
        &self,
        conversation_id: ConversationId,
    ) -> Arc<std::sync::Mutex<sober_skill::SkillActivationState>> {
        // Try read first
        {
            let activations = self
                .skill_activations
                .read()
                .expect("skill activations lock");
            if let Some(state) = activations.get(&conversation_id) {
                return Arc::clone(state);
            }
        }
        // Create new
        let mut activations = self
            .skill_activations
            .write()
            .expect("skill activations lock");
        let state = activations.entry(conversation_id).or_insert_with(|| {
            Arc::new(std::sync::Mutex::new(
                sober_skill::SkillActivationState::default(),
            ))
        });
        Arc::clone(state)
    }

    /// Removes the skill activation state for a conversation.
    /// Call when a conversation ends or is deleted.
    pub fn clear_skill_activations(&self, conversation_id: ConversationId) {
        if let Ok(mut activations) = self.skill_activations.write() {
            activations.remove(&conversation_id);
        }
    }

    /// Resolves which conversation to deliver job results to.
    ///
    /// Tries the original `conversation_id` first; falls back to the user's
    /// most recent conversation in the same workspace.
    pub async fn resolve_delivery_conversation(
        &self,
        conversation_id: Option<ConversationId>,
        user_id: UserId,
        workspace_id: Option<WorkspaceId>,
    ) -> Option<ConversationId> {
        // Try the original conversation first
        if let Some(cid) = conversation_id
            && self.repos.conversations().get_by_id(cid).await.is_ok()
        {
            return Some(cid);
        }
        // Fallback: user's most recent conversation in the same workspace
        self.repos
            .conversations()
            .find_latest_by_user_and_workspace(user_id, workspace_id)
            .await
            .ok()
            .flatten()
            .map(|c| c.id)
    }

    /// Attempts to resolve a dynamic LLM engine from user-stored keys.
    ///
    /// Returns `Some(engine)` if the user has a stored `llm_provider` secret
    /// that decrypts successfully and differs from the system default.
    /// Returns `None` if no user key is found, decryption fails, or no MEK
    /// is available — in which case the static engine should be used.
    #[allow(dead_code)]
    pub(crate) async fn try_resolve_dynamic_engine(
        repos: &Arc<R>,
        user_id: UserId,
        conversation_id: ConversationId,
        llm_config: &Option<LlmConfig>,
        mek: &Option<Arc<Mek>>,
    ) -> Option<OpenAiCompatibleEngine> {
        use sober_llm::LlmKeyResolver;

        let config = llm_config.as_ref()?;
        let mek = mek.as_ref()?;

        let resolver =
            LlmKeyResolver::new(repos.secrets().clone(), Arc::clone(mek), config.clone());

        let resolved = match resolver.resolve(user_id, Some(conversation_id)).await {
            Ok(key) => key,
            Err(e) => {
                warn!(error = %e, "LLM key resolution failed, using static engine");
                return None;
            }
        };

        // System fallback — no need to create a dynamic engine.
        if resolved.provider == "system" {
            return None;
        }

        debug!(
            user_id = %user_id,
            provider = %resolved.provider,
            "using dynamically resolved LLM key"
        );

        Some(OpenAiCompatibleEngine::new(
            resolved.base_url,
            Some(resolved.api_key),
            &config.model,
            &config.embedding_model,
            config.max_tokens,
        ))
    }

    // -----------------------------------------------------------------------
    // handle_message — actor-routed entry point
    // -----------------------------------------------------------------------

    /// Handles a user message by routing it through the actor registry.
    ///
    /// If a [`ConversationActor`] already exists for this conversation, the
    /// message is sent to its inbox. Otherwise a new actor is spawned. Returns
    /// a stream of [`AgentEvent`]s for the caller to consume.
    pub async fn handle_message(
        self: &Arc<Self>,
        user_id: UserId,
        conversation_id: ConversationId,
        content: &[sober_core::types::ContentBlock],
        trigger: TriggerKind,
    ) -> Result<AgentResponseStream, AgentError> {
        // 1. Pre-flight injection check (before routing to actor).
        let text_content = ContentBlock::extract_text(content);
        let verdict = Mind::check_injection(&text_content);
        match verdict {
            InjectionVerdict::Rejected { reason } => {
                return Err(AgentError::InjectionDetected(reason));
            }
            InjectionVerdict::Flagged { reason } => {
                warn!(user_id = %user_id, reason = %reason, "input flagged for injection");
            }
            InjectionVerdict::Pass => {}
        }

        // 2. Create per-message event channel.
        let (event_tx, event_rx) =
            mpsc::channel::<Result<AgentEvent, AgentError>>(EVENT_CHANNEL_BUFFER);

        // 3. Get or spawn actor for this conversation.
        let inbox_tx = if let Some(tx) = self.registry.get_sender(&conversation_id) {
            tx
        } else {
            // Spawn a new actor.
            let (inbox_tx, inbox_rx) = mpsc::channel(ACTOR_INBOX_BUFFER);
            let ctx = AgentContext {
                llm: Arc::clone(&self.llm),
                mind: Arc::clone(&self.mind),
                memory: Arc::clone(&self.memory),
                context_loader: Arc::clone(&self.context_loader),
                repos: Arc::clone(&self.repos),
                config: self.config.clone(),
                memory_config: self.memory_config.clone(),
                registrar: self.registrar.clone(),
                broadcast_tx: self.broadcast_tx.clone(),
                mek: self.mek.clone(),
                llm_config: self.llm_config.clone(),
                tool_bootstrap: Arc::clone(&self.tool_bootstrap),
                static_tools: self.static_tools.clone(),
            };
            let actor = ConversationActor::new(
                conversation_id,
                inbox_rx,
                ctx,
                self.skill_activation_state(conversation_id),
            );

            let registry = self.registry.clone();
            let cid = conversation_id;
            tokio::spawn(async move {
                actor.run().await;
                registry.remove(&cid);
            });

            self.registry.insert(
                conversation_id,
                ActorHandle {
                    inbox_tx: inbox_tx.clone(),
                },
            );
            inbox_tx
        };

        // 4. Send the message to the actor's inbox.
        let msg = InboxMessage::UserMessage {
            user_id,
            content: content.to_vec(),
            trigger,
            event_tx,
        };
        if inbox_tx.send(msg).await.is_err() {
            // Actor exited between the registry lookup and the send.
            // Remove the stale entry and return an error.
            self.registry.remove(&conversation_id);
            return Err(AgentError::Internal(
                "conversation actor exited unexpectedly".to_owned(),
            ));
        }

        // 5. Return the event stream.
        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(
            event_rx,
        )))
    }

    // -----------------------------------------------------------------------
    // Graceful shutdown
    // -----------------------------------------------------------------------

    /// Gracefully shuts down all active conversation actors.
    ///
    /// Sends [`InboxMessage::Shutdown`] to every actor and waits briefly for
    /// them to drain their current work.
    pub async fn shutdown(&self) {
        self.registry.shutdown_all().await;
    }
}

// ---------------------------------------------------------------------------
// Standalone helpers
// ---------------------------------------------------------------------------

/// Formats loaded memory context into a human-readable string for inclusion
/// in the task context.
pub fn format_memory_context(context: &LoadedContext) -> String {
    let mut parts = Vec::new();

    for hit in &context.conversation_memories {
        parts.push(format!(
            "- [conversation memory, score={:.2}] {}",
            hit.score, hit.content
        ));
    }

    for hit in &context.user_memories {
        parts.push(format!(
            "- [user memory, score={:.2}] {}",
            hit.score, hit.content
        ));
    }

    for hit in &context.system_memories {
        parts.push(format!(
            "- [system memory, score={:.2}] {}",
            hit.score, hit.content
        ));
    }

    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_config_defaults() {
        let config = AgentConfig::default();
        assert_eq!(config.max_tool_iterations, 25);
        assert_eq!(config.context_token_budget, 128_000);
        assert_eq!(config.conversation_history_limit, 200);
        assert_eq!(config.hits_per_scope, 5);
        assert_eq!(config.model, "anthropic/claude-sonnet-4");
        assert_eq!(config.embedding_model, "text-embedding-3-small");
        assert_eq!(config.max_tokens, 4096);
    }

    #[test]
    fn format_memory_context_empty() {
        let context = LoadedContext {
            recent_messages: vec![],
            conversation_memories: vec![],
            user_memories: vec![],
            system_memories: vec![],
            estimated_tokens: 0,
        };
        assert!(format_memory_context(&context).is_empty());
    }

    #[test]
    fn format_memory_context_with_hits() {
        use sober_memory::MemoryHit;

        let context = LoadedContext {
            recent_messages: vec![],
            conversation_memories: vec![],
            user_memories: vec![MemoryHit {
                point_id: uuid::Uuid::nil(),
                content: "User prefers Rust.".to_owned(),
                chunk_type: sober_memory::ChunkType::Fact,
                scope_id: sober_core::ScopeId::new(),
                source_message_id: None,
                importance: 0.8,
                score: 0.95,
                created_at: chrono::Utc::now(),
            }],
            system_memories: vec![MemoryHit {
                point_id: uuid::Uuid::nil(),
                content: "System knowledge.".to_owned(),
                chunk_type: sober_memory::ChunkType::Fact,
                scope_id: sober_core::ScopeId::GLOBAL,
                source_message_id: None,
                importance: 1.0,
                score: 0.80,
                created_at: chrono::Utc::now(),
            }],
            estimated_tokens: 100,
        };

        let formatted = format_memory_context(&context);
        assert!(formatted.contains("user memory"));
        assert!(formatted.contains("User prefers Rust."));
        assert!(formatted.contains("system memory"));
        assert!(formatted.contains("System knowledge."));
    }
}
