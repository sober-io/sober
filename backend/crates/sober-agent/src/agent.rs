//! Core agent logic — the agentic loop that handles user messages.
//!
//! The [`Agent`] struct orchestrates the full message-handling flow:
//! injection detection → context loading → prompt assembly → LLM completion
//! → tool execution → response streaming.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use sober_core::config::{LlmConfig, MemoryConfig};
use sober_core::types::AgentRepos;
use sober_core::types::access::{CallerContext, TriggerKind};
use sober_core::types::domain::Message as DomainMessage;
use sober_core::types::enums::{AgentMode, ConversationKind, MessageRole};
use sober_core::types::ids::{ConversationId, UserId, WorkspaceId};
use sober_core::types::input::CreateMessage;
use sober_core::types::repo::{ConversationRepo, MessageRepo, UserRepo, WorkspaceRepo};
use sober_crypto::envelope::Mek;
use sober_llm::types::{CompletionRequest, ToolCall as LlmToolCall};
use sober_llm::{LlmEngine, Message as LlmMessage, OpenAiCompatibleEngine};
use sober_memory::{ContextLoader, LoadRequest, LoadedContext, MemoryStore, StoreChunk};
use sober_mind::assembly::{Mind, TaskContext};
use sober_mind::injection::InjectionVerdict;

use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::broadcast::ConversationUpdateSender;
use crate::confirm::ConfirmationRegistrar;
use crate::error::AgentError;
use crate::grpc::proto;
use crate::stream::{AgentEvent, AgentResponseStream, Usage};
use crate::tools::{ToolBootstrap, ToolRegistry, TurnContext};

/// Buffer size for the agent event channel.
const EVENT_CHANNEL_BUFFER: usize = 64;

/// Timeout in seconds for user confirmation of dangerous commands.
const CONFIRM_TIMEOUT_SECS: u64 = 300;

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
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_tool_iterations: 25,
            context_token_budget: 128_000,
            conversation_history_limit: 50,
            hits_per_scope: 5,
            model: "anthropic/claude-sonnet-4".to_owned(),
            embedding_model: "text-embedding-3-small".to_owned(),
            max_tokens: 4096,
        }
    }
}

/// Details of a confirmation request extracted from [`ToolError::NeedsConfirmation`].
struct ConfirmDetail {
    confirm_id: String,
    command: String,
    risk_level: String,
    reason: String,
}

/// Aggregated context passed to the agentic loop, avoiding long parameter lists.
struct LoopContext<'a, R: AgentRepos> {
    llm: &'a Arc<dyn LlmEngine>,
    mind: &'a Arc<Mind>,
    memory: &'a Arc<MemoryStore>,
    context_loader: &'a Arc<ContextLoader<R::Msg>>,
    tool_registry: &'a Arc<ToolRegistry>,
    repos: &'a Arc<R>,
    config: &'a AgentConfig,
    memory_config: &'a MemoryConfig,
    user_id: UserId,
    conversation_id: ConversationId,
    content: &'a str,
    user_msg_id: sober_core::MessageId,
    event_tx: &'a mpsc::Sender<Result<AgentEvent, AgentError>>,
    broadcast_tx: &'a ConversationUpdateSender,
    registrar: Option<&'a ConfirmationRegistrar>,
    /// What triggered this interaction — controls storage and tool behavior.
    trigger: TriggerKind,
    /// The kind of conversation (direct vs group) — used for prompt assembly.
    conversation_kind: ConversationKind,
    /// Workspace ID from the conversation (for audit logging).
    workspace_id: Option<WorkspaceId>,
    /// Resolved workspace directory path (for LLM context).
    workspace_dir: Option<PathBuf>,
    /// LLM config for constructing dynamic engines from resolved user keys.
    llm_config: &'a Option<LlmConfig>,
    /// Master encryption key for resolving user-stored LLM keys.
    mek: &'a Option<Arc<Mek>>,
    /// XML listing of all available skills for system prompt injection.
    skill_catalog_xml: String,
}

/// The core agent that handles messages through an agentic loop.
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

    /// Builds a [`ToolRegistry`] for the given conversation turn via the
    /// centralized [`ToolBootstrap`].
    ///
    /// Reuses the pre-built [`static_tools`](Self::static_tools) and adds
    /// per-conversation tools (shell, secrets, artifacts, snapshots, skills).
    async fn build_turn_tools(
        &self,
        user_id: UserId,
        conversation_id: ConversationId,
        workspace_id: Option<WorkspaceId>,
        workspace_dir: Option<PathBuf>,
    ) -> Arc<ToolRegistry> {
        let ctx = TurnContext {
            user_id,
            conversation_id,
            workspace_id,
            workspace_dir,
            skill_activation_state: Some(self.skill_activation_state(conversation_id)),
        };
        Arc::new(self.tool_bootstrap.build(&ctx, &self.static_tools).await)
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
    async fn try_resolve_dynamic_engine(
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

    /// Handles a user message through the full agentic loop.
    ///
    /// Returns a stream of [`AgentEvent`]s that are emitted in real-time
    /// as the agent processes the message (enabling confirmation flow and
    /// progressive tool output).
    pub async fn handle_message(
        &self,
        user_id: UserId,
        conversation_id: ConversationId,
        content: &str,
        trigger: TriggerKind,
    ) -> Result<AgentResponseStream, AgentError> {
        // 1. Check injection
        let verdict = Mind::check_injection(content);
        match verdict {
            InjectionVerdict::Rejected { reason } => {
                return Err(AgentError::InjectionDetected(reason));
            }
            InjectionVerdict::Flagged { reason } => {
                warn!(user_id = %user_id, reason = %reason, "input flagged for injection");
            }
            InjectionVerdict::Pass => {}
        }

        // 2. Resolve conversation and workspace.
        //    Must happen before slash interception (workspace-level skills need the path).
        let conversation = self
            .repos
            .conversations()
            .get_by_id(conversation_id)
            .await
            .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;

        // Resolve or provision workspace for this conversation.
        let workspace_root_env = std::env::var("WORKSPACE_ROOT")
            .unwrap_or_else(|_| "/var/lib/sober/workspaces".to_string());

        // If conversation has no workspace, create one.
        // Each conversation gets its own workspace: {WORKSPACE_ROOT}/{conversation_id}/
        let mut conversation = conversation;
        if conversation.workspace_id.is_none() {
            let root_path = format!("{}/{}", workspace_root_env, conversation_id);
            match self
                .repos
                .workspaces()
                .create(user_id, &conversation_id.to_string(), None, &root_path)
                .await
            {
                Ok(ws) => {
                    // Link workspace to conversation.
                    if let Err(e) = self
                        .repos
                        .conversations()
                        .update_workspace(conversation_id, Some(ws.id))
                        .await
                    {
                        warn!("failed to link workspace to conversation: {e}");
                    } else {
                        conversation.workspace_id = Some(ws.id);
                    }
                }
                Err(e) => {
                    warn!("failed to create workspace: {e}");
                }
            }
        }

        // Resolve workspace directory for shell tool cwd.
        // Each conversation's workspace root IS the working directory (no subdir nesting).
        let workspace_dir = if let Some(ws_id) = conversation.workspace_id {
            match self.repos.workspaces().get_by_id(ws_id).await {
                Ok(ws) => {
                    let root = PathBuf::from(&ws.root_path);
                    match tokio::fs::create_dir_all(&root).await {
                        Ok(()) => Some(root),
                        Err(e) => {
                            warn!(
                                workspace_id = %ws_id,
                                root_path = %ws.root_path,
                                "failed to create workspace dir: {e}"
                            );
                            None
                        }
                    }
                }
                Err(_) => None,
            }
        } else {
            None
        };

        // 3. Store user message (only for human-driven calls).
        let user_msg_id = if trigger == TriggerKind::Human {
            let user_msg = self
                .repos
                .messages()
                .create(CreateMessage {
                    conversation_id,
                    role: MessageRole::User,
                    content: content.to_owned(),
                    tool_calls: None,
                    tool_result: None,
                    token_count: None,
                    metadata: None,
                    user_id: Some(user_id),
                })
                .await
                .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;
            user_msg.id
        } else {
            sober_core::MessageId::new()
        };

        // Load skill catalog XML for system prompt injection.
        let skill_catalog_xml = {
            let user_home = sober_workspace::user_home_dir();
            let ws_path = workspace_dir.clone().unwrap_or_default();
            match self
                .tool_bootstrap
                .skill_loader
                .load(&user_home, &ws_path)
                .await
            {
                Ok(catalog) => catalog.to_catalog_xml(),
                Err(e) => {
                    warn!(error = %e, "failed to load skill catalog for prompt injection");
                    String::new()
                }
            }
        };

        // 5. Check agent mode — decide whether to run the LLM pipeline
        let should_respond = match conversation.agent_mode {
            AgentMode::Always => true,
            AgentMode::Silent => false,
            AgentMode::Mention => content.to_lowercase().contains("@sober"),
        };

        if !should_respond {
            debug!(
                agent_mode = ?conversation.agent_mode,
                conversation_id = %conversation_id,
                "skipping LLM pipeline due to agent mode"
            );
            // Return an empty stream — message was stored but no agent response.
            let (event_tx, event_rx) = mpsc::channel::<Result<AgentEvent, AgentError>>(1);
            let _ = event_tx
                .send(Ok(AgentEvent::Done {
                    message_id: user_msg_id,
                    usage: Usage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                    },
                    artifact_ref: None,
                }))
                .await;
            return Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(
                event_rx,
            )));
        }

        // 4. Build per-conversation tools via ToolBootstrap.
        let tool_registry = self
            .build_turn_tools(
                user_id,
                conversation_id,
                conversation.workspace_id,
                workspace_dir.clone(),
            )
            .await;

        // 5. Create event channel and spawn the agentic loop
        let (event_tx, event_rx) =
            mpsc::channel::<Result<AgentEvent, AgentError>>(EVENT_CHANNEL_BUFFER);

        // Clone what we need for the spawned task.
        let llm = Arc::clone(&self.llm);
        let mind = Arc::clone(&self.mind);
        let memory = Arc::clone(&self.memory);
        let context_loader = Arc::clone(&self.context_loader);
        let repos = Arc::clone(&self.repos);
        let config = self.config.clone();
        let memory_config = self.memory_config.clone();
        let registrar = self.registrar.clone();
        let broadcast_tx = self.broadcast_tx.clone();
        let content = content.to_owned();
        let conversation_kind = conversation.kind;
        let workspace_id = conversation.workspace_id;
        let llm_config = self.llm_config.clone();
        let mek = self.mek.clone();

        let trigger_label = format!("{trigger:?}").to_lowercase();

        tokio::spawn(async move {
            let request_start = Instant::now();

            let ctx = LoopContext {
                llm: &llm,
                mind: &mind,
                memory: &memory,
                context_loader: &context_loader,
                tool_registry: &tool_registry,
                repos: &repos,
                config: &config,
                memory_config: &memory_config,
                user_id,
                conversation_id,
                content: &content,
                user_msg_id,
                event_tx: &event_tx,
                broadcast_tx: &broadcast_tx,
                registrar: registrar.as_ref(),
                trigger,
                conversation_kind,
                workspace_id,
                workspace_dir,
                llm_config: &llm_config,
                mek: &mek,
                skill_catalog_xml,
            };
            let result = Self::run_loop_streaming(&ctx).await;

            let status_label = if result.is_ok() { "success" } else { "error" };
            metrics::counter!(
                "sober_agent_requests_total",
                "trigger" => trigger_label.clone(),
                "status" => status_label,
            )
            .increment(1);
            metrics::histogram!("sober_agent_request_duration_seconds",
                "trigger" => trigger_label,
            )
            .record(request_start.elapsed().as_secs_f64());

            if let Err(e) = result {
                let error_msg = e.to_string();
                let _ = event_tx.send(Err(e)).await;
                let _ = broadcast_tx.send(crate::grpc::proto::ConversationUpdate {
                    conversation_id: conversation_id.to_string(),
                    event: Some(crate::grpc::proto::conversation_update::Event::Error(
                        crate::grpc::proto::Error { message: error_msg },
                    )),
                });
            }
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(
            event_rx,
        )))
    }

    /// Streaming agentic loop that sends events through a channel in real-time.
    ///
    /// This is the core loop: embed → load context → assemble → complete →
    /// handle tool calls → repeat. Events are sent immediately as they occur,
    /// enabling confirmation flow and progressive output.
    async fn run_loop_streaming(ctx: &LoopContext<'_, R>) -> Result<(), AgentError> {
        let llm = ctx.llm;
        let mind = ctx.mind;
        let memory = ctx.memory;
        let context_loader = ctx.context_loader;
        let tool_registry = ctx.tool_registry;
        let repos = ctx.repos;
        let config = ctx.config;
        let memory_config = ctx.memory_config;
        let user_id = ctx.user_id;
        let conversation_id = ctx.conversation_id;
        let content = ctx.content;
        let _user_msg_id = ctx.user_msg_id;
        let event_tx = ctx.event_tx;
        let broadcast_tx = ctx.broadcast_tx;
        let registrar = ctx.registrar;
        let conv_id_str = conversation_id.to_string();
        let mut llm_messages: Vec<LlmMessage> = Vec::new();
        let mut needs_rebuild = true;
        let mut base_message_count: usize = 0;
        let mut consecutive_tool_errors: u32 = 0;
        let mut assistant_text = String::new();
        const MAX_CONSECUTIVE_TOOL_ERRORS: u32 = 3;

        for iteration in 0..config.max_tool_iterations {
            debug!(iteration, "agent loop iteration");
            metrics::counter!("sober_agent_loop_iterations_total").increment(1);

            if needs_rebuild {
                // a. Embed user message for context retrieval
                let query_vector = match llm.embed(&[content]).await {
                    Ok(vecs) => vecs.into_iter().next().unwrap_or_default(),
                    Err(e) => {
                        warn!(error = %e, "embedding failed, proceeding without memory context");
                        vec![]
                    }
                };

                // b. Load context
                let loaded_context = if query_vector.is_empty() {
                    context_loader
                        .load(
                            LoadRequest {
                                query_vector: vec![],
                                query_text: content.to_owned(),
                                user_id,
                                conversation_id,
                                token_budget: config.context_token_budget,
                                recent_message_count: if ctx.trigger == TriggerKind::Human {
                                    config.conversation_history_limit
                                } else {
                                    0
                                },
                                hits_per_scope: 0,
                            },
                            memory_config,
                        )
                        .await
                        .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?
                } else {
                    context_loader
                        .load(
                            LoadRequest {
                                query_vector,
                                query_text: content.to_owned(),
                                user_id,
                                conversation_id,
                                token_budget: config.context_token_budget,
                                recent_message_count: if ctx.trigger == TriggerKind::Human {
                                    config.conversation_history_limit
                                } else {
                                    0
                                },
                                hits_per_scope: config.hits_per_scope,
                            },
                            memory_config,
                        )
                        .await
                        .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?
                };

                // c. Assemble prompt via Mind
                let caller = CallerContext {
                    user_id: Some(user_id),
                    trigger: TriggerKind::Human,
                    permissions: vec![],
                    scope_grants: vec![],
                    workspace_id: None,
                };

                // Build user display names for group conversations so the
                // mind can prefix messages with `[username]:`.
                let user_display_names = if ctx.conversation_kind == ConversationKind::Group {
                    let user_ids: HashSet<UserId> = loaded_context
                        .recent_messages
                        .iter()
                        .filter_map(|m| m.user_id)
                        .collect();
                    let mut names = HashMap::new();
                    for uid in user_ids {
                        if let Ok(user) = repos.users().get_by_id(uid).await {
                            names.insert(uid, user.username);
                        }
                    }
                    names
                } else {
                    HashMap::new()
                };

                let memory_context_text = format_memory_context(&loaded_context);
                let mut task_description = String::new();

                // Include workspace path so the LLM knows where it's working.
                if let Some(ref ws_dir) = ctx.workspace_dir {
                    task_description.push_str(&format!(
                        "## Workspace\n\nCurrent workspace directory: `{}`\n\n",
                        ws_dir.display()
                    ));
                }

                if !memory_context_text.is_empty() {
                    task_description
                        .push_str(&format!("## Relevant Memories\n\n{memory_context_text}"));
                }

                let task_context = TaskContext {
                    description: task_description,
                    recent_messages: loaded_context.recent_messages.clone(),
                    conversation_kind: ctx.conversation_kind,
                    user_display_names,
                };

                let tool_metadata = tool_registry.tool_metadata();
                let assembled = mind
                    .assemble(
                        &caller,
                        &task_context,
                        &tool_metadata,
                        "",
                        &ctx.skill_catalog_xml,
                    )
                    .await
                    .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;

                // d. Convert domain messages to LLM messages, preserving tool
                //    call history from previous iterations so the LLM doesn't
                //    repeat already-executed tool calls after a context rebuild.
                let new_base = domain_to_llm_messages(&assembled);
                let tool_history: Vec<LlmMessage> =
                    if base_message_count > 0 && llm_messages.len() > base_message_count {
                        llm_messages.split_off(base_message_count)
                    } else {
                        Vec::new()
                    };
                base_message_count = new_base.len();
                llm_messages = new_base;
                // For non-human triggers the prompt wasn't stored in the DB,
                // so the context loader didn't include it. Add it explicitly.
                if ctx.trigger != TriggerKind::Human {
                    llm_messages.push(LlmMessage::user(content));
                }
                llm_messages.extend(tool_history);
                needs_rebuild = false;
            }

            // e. Call LLM — exclude tools that shouldn't be available:
            //    - scheduler tool excluded for scheduler-driven calls (prevent job recreation)
            let tool_definitions = if ctx.trigger == TriggerKind::Scheduler {
                tool_registry.tool_definitions_except(&["scheduler"])
            } else {
                tool_registry.tool_definitions()
            };
            let req = CompletionRequest {
                model: config.model.clone(),
                messages: llm_messages.clone(),
                tools: tool_definitions,
                max_tokens: Some(config.max_tokens),
                temperature: None,
                stop: vec![],
                stream: false,
            };

            // Try dynamic key resolution: if the user has stored an LLM key,
            // construct a temporary engine with those credentials.
            let dynamic_engine: Option<OpenAiCompatibleEngine> = Self::try_resolve_dynamic_engine(
                repos,
                user_id,
                conversation_id,
                ctx.llm_config,
                ctx.mek,
            )
            .await;
            let effective_llm: &dyn LlmEngine = match dynamic_engine {
                Some(ref engine) => engine,
                None => &**llm,
            };

            let response = effective_llm
                .complete(req)
                .await
                .map_err(|e| AgentError::LlmCallFailed(e.to_string()))?;

            let choice =
                response.choices.into_iter().next().ok_or_else(|| {
                    AgentError::LlmCallFailed("no choices in response".to_owned())
                })?;

            let usage_stats = response.usage;

            // f. Check for tool calls
            if let Some(ref tool_calls) = choice.message.tool_calls
                && !tool_calls.is_empty()
            {
                llm_messages.push(LlmMessage {
                    role: "assistant".to_owned(),
                    content: choice.message.content.clone(),
                    reasoning_content: choice.message.reasoning_content.clone(),
                    tool_calls: choice.message.tool_calls.clone(),
                    tool_call_id: None,
                });

                // Store assistant message with tool calls before executing them.
                // Skip DB storage when all tool calls are for internal tools
                // (e.g. activate_skill) — they stay in-memory only.
                let all_internal = tool_calls.iter().all(|tc| {
                    tool_registry
                        .get_tool(&tc.function.name)
                        .is_some_and(|t| t.metadata().internal)
                });

                if !all_internal {
                    let tool_calls_json = serde_json::to_value(&choice.message.tool_calls).ok();
                    let msg_metadata = choice
                        .message
                        .reasoning_content
                        .as_ref()
                        .map(|rc| serde_json::json!({ "reasoning_content": rc }));
                    repos
                        .messages()
                        .create(CreateMessage {
                            conversation_id,
                            role: MessageRole::Assistant,
                            content: choice.message.content.clone().unwrap_or_default(),
                            tool_calls: tool_calls_json,
                            tool_result: None,
                            token_count: usage_stats.as_ref().map(|u| u.total_tokens as i32),
                            metadata: msg_metadata,
                            user_id: Some(user_id),
                        })
                        .await
                        .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;
                }

                let (tool_msgs, any_context_modifying, had_errors) =
                    Self::execute_tool_calls_streaming(
                        tool_registry,
                        tool_calls,
                        event_tx,
                        broadcast_tx,
                        &conv_id_str,
                        registrar,
                        user_id,
                        conversation_id,
                        repos,
                        ctx.workspace_id,
                    )
                    .await;

                llm_messages.extend(tool_msgs);

                if any_context_modifying {
                    needs_rebuild = true;
                }

                if had_errors {
                    consecutive_tool_errors += 1;
                    if consecutive_tool_errors >= MAX_CONSECUTIVE_TOOL_ERRORS {
                        warn!(
                            consecutive_errors = consecutive_tool_errors,
                            "tool error circuit breaker tripped, forcing text response"
                        );
                        llm_messages.push(LlmMessage::user(
                            "SYSTEM: Tool calls have failed multiple times in a row. \
                             Stop calling tools and respond to the user with what you know. \
                             Explain that the tool execution failed and why.",
                        ));
                    }
                } else {
                    consecutive_tool_errors = 0;
                }
                continue;
            }

            // g. Text response — strip memory extractions before storing/broadcasting.
            let raw_text = choice.message.content.unwrap_or_default();
            let extraction_result = crate::extraction::strip_extractions(&raw_text);
            let text = extraction_result.cleaned_text;

            if !text.is_empty() {
                let _ = event_tx.send(Ok(AgentEvent::TextDelta(text.clone()))).await;
                let _ = broadcast_tx.send(proto::ConversationUpdate {
                    conversation_id: conv_id_str.clone(),
                    event: Some(proto::conversation_update::Event::TextDelta(
                        proto::TextDelta {
                            content: text.clone(),
                        },
                    )),
                });
                assistant_text = text.clone();
            }

            // Always store the assistant response (cleaned, without extraction block).
            let assistant_msg = repos
                .messages()
                .create(CreateMessage {
                    conversation_id,
                    role: MessageRole::Assistant,
                    content: text.clone(),
                    tool_calls: None,
                    tool_result: None,
                    token_count: usage_stats.map(|u| u.total_tokens as i32),
                    metadata: None,
                    user_id: Some(user_id),
                })
                .await
                .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;

            // Store inline memory extractions (replaces raw conversation ingestion).
            if !extraction_result.extractions.is_empty() {
                Self::spawn_extraction_ingestion(
                    llm,
                    memory,
                    user_id,
                    extraction_result.extractions,
                    memory_config.decay_half_life_days,
                );
            }
            let assistant_msg_id = assistant_msg.id;

            // Broadcast NewMessage for the assistant message.
            let _ = broadcast_tx.send(proto::ConversationUpdate {
                conversation_id: conv_id_str.clone(),
                event: Some(proto::conversation_update::Event::NewMessage(
                    proto::NewMessage {
                        message_id: assistant_msg_id.to_string(),
                        role: "Assistant".to_owned(),
                        content: text.clone(),
                        source: format!("{:?}", ctx.trigger).to_lowercase(),
                        user_id: Some(user_id.to_string()),
                    },
                )),
            });

            let _ = event_tx
                .send(Ok(AgentEvent::Done {
                    message_id: assistant_msg_id,
                    usage: Usage {
                        prompt_tokens: usage_stats.map_or(0, |u| u.prompt_tokens),
                        completion_tokens: usage_stats.map_or(0, |u| u.completion_tokens),
                    },
                    artifact_ref: None,
                }))
                .await;
            let _ = broadcast_tx.send(proto::ConversationUpdate {
                conversation_id: conv_id_str.clone(),
                event: Some(proto::conversation_update::Event::Done(proto::Done {
                    message_id: assistant_msg_id.to_string(),
                    prompt_tokens: usage_stats.map_or(0, |u| u.prompt_tokens),
                    completion_tokens: usage_stats.map_or(0, |u| u.completion_tokens),
                    artifact_ref: String::new(),
                })),
            });

            // i. Auto-generate title
            let conversation = repos.conversations().get_by_id(conversation_id).await.ok();
            let current_title = conversation.as_ref().and_then(|c| c.title.as_deref());
            let needs_title = current_title.is_none() || current_title == Some("");
            if needs_title && !assistant_text.is_empty() {
                let title_prompt = format!(
                    "Generate a short title (max 6 words) for a conversation that starts with:\n\
                     User: {content}\n\
                     Assistant: {}\n\n\
                     Reply with ONLY the title, no quotes or punctuation at the end.",
                    &assistant_text[..assistant_text.len().min(200)]
                );

                let title_req = CompletionRequest {
                    model: config.model.clone(),
                    messages: vec![LlmMessage {
                        role: "user".to_owned(),
                        content: Some(title_prompt),
                        reasoning_content: None,
                        tool_calls: None,
                        tool_call_id: None,
                    }],
                    tools: vec![],
                    max_tokens: Some(200),
                    temperature: Some(0.3),
                    stop: vec![],
                    stream: false,
                };

                if let Ok(title_resp) = llm.complete(title_req).await
                    && let Some(title) = title_resp.choices.into_iter().next().and_then(|c| {
                        // Primary: use content field if non-empty
                        let title = c
                            .message
                            .content
                            .filter(|s| !s.trim().is_empty())
                            .map(|t| t.trim().trim_matches('"').to_owned());

                        if title.is_some() {
                            return title;
                        }

                        // Fallback for thinking models: extract title from reasoning content
                        c.message.reasoning_content.and_then(|r| {
                            r.lines()
                                .filter_map(|line| {
                                    line.strip_prefix(" - ").or(line.strip_prefix("- "))
                                })
                                .find(|candidate| {
                                    let words = candidate.split_whitespace().count();
                                    (2..=6).contains(&words)
                                })
                                .map(|t| t.trim().trim_matches('"').to_owned())
                        })
                    })
                    && repos
                        .conversations()
                        .update_title(conversation_id, &title)
                        .await
                        .is_ok()
                {
                    let _ = event_tx
                        .send(Ok(AgentEvent::TitleGenerated(title.clone())))
                        .await;
                    let _ = broadcast_tx.send(proto::ConversationUpdate {
                        conversation_id: conv_id_str.clone(),
                        event: Some(proto::conversation_update::Event::TitleChanged(
                            proto::TitleChanged { title },
                        )),
                    });
                }
            }

            metrics::histogram!("sober_agent_loop_iterations_per_request")
                .record(f64::from(iteration + 1));
            return Ok(());
        }

        metrics::histogram!("sober_agent_loop_iterations_per_request")
            .record(f64::from(config.max_tool_iterations));
        Err(AgentError::MaxIterationsExceeded(
            config.max_tool_iterations,
        ))
    }

    /// Executes tool calls, sending events through the channel in real-time.
    ///
    /// Handles [`ToolError::NeedsConfirmation`] by sending a confirmation
    /// request to the client and waiting for the user's response via the
    /// broker. If approved, re-executes the tool with `"confirmed": true`.
    ///
    /// Returns (LLM messages, context_modifying, any_errors).
    #[allow(clippy::too_many_arguments)]
    async fn execute_tool_calls_streaming(
        tool_registry: &Arc<ToolRegistry>,
        tool_calls: &[LlmToolCall],
        event_tx: &mpsc::Sender<Result<AgentEvent, AgentError>>,
        broadcast_tx: &ConversationUpdateSender,
        conv_id_str: &str,
        registrar: Option<&ConfirmationRegistrar>,
        user_id: UserId,
        conversation_id: ConversationId,
        repos: &Arc<R>,
        workspace_id: Option<WorkspaceId>,
    ) -> (Vec<LlmMessage>, bool, bool) {
        use sober_core::types::tool::ToolError;

        let mut messages = Vec::new();
        let mut any_context_modifying = false;
        let mut any_errors = false;

        for tc in tool_calls {
            let tool_name = &tc.function.name;
            let mut tool_input: serde_json::Value =
                match serde_json::from_str(&tc.function.arguments) {
                    Ok(v) => v,
                    Err(e) => {
                        warn!(
                            tool = %tool_name,
                            error = %e,
                            "failed to parse tool call arguments from LLM, using null"
                        );
                        serde_json::Value::Null
                    }
                };

            // Inject caller context into tool input so tools can authorize
            // and associate results with the originating conversation.
            if let serde_json::Value::Object(ref mut map) = tool_input {
                map.entry("owner_id")
                    .or_insert_with(|| serde_json::Value::String(user_id.to_string()));
                map.entry("conversation_id")
                    .or_insert_with(|| serde_json::Value::String(conversation_id.to_string()));
                // TODO: resolve from RoleRepo when RBAC is wired into the agent
                map.entry("is_admin")
                    .or_insert(serde_json::Value::Bool(false));
            }

            let tool_internal = tool_registry
                .get_tool(tool_name)
                .is_some_and(|t| t.metadata().internal);

            // Skip broadcasting ToolCallStart for internal tools to prevent
            // leaking secret data (e.g. store_secret arguments) to clients.
            if !tool_internal {
                let _ = event_tx
                    .send(Ok(AgentEvent::ToolCallStart {
                        name: tool_name.clone(),
                        input: tool_input.clone(),
                    }))
                    .await;
                let _ = broadcast_tx.send(proto::ConversationUpdate {
                    conversation_id: conv_id_str.to_owned(),
                    event: Some(proto::conversation_update::Event::ToolCallStart(
                        proto::ToolCallStart {
                            name: tool_name.clone(),
                            input_json: tool_input.to_string(),
                            internal: false,
                        },
                    )),
                });
            }

            // Save the command for audit logging before tool_input is moved.
            let shell_command = if tool_name == "shell" {
                tool_input
                    .get("command")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            } else {
                None
            };

            let tool_start = Instant::now();
            let output = match tool_registry.get_tool(tool_name) {
                Some(tool) => {
                    if tool.metadata().context_modifying {
                        any_context_modifying = true;
                    }
                    match tool.execute(tool_input.clone()).await {
                        Ok(output) => {
                            metrics::counter!(
                                "sober_agent_tool_calls_total",
                                "tool" => tool_name.clone(),
                                "status" => "success",
                            )
                            .increment(1);
                            output.content
                        }
                        Err(ToolError::NeedsConfirmation {
                            confirm_id,
                            command,
                            risk_level,
                            reason,
                        }) => {
                            let detail = ConfirmDetail {
                                confirm_id,
                                command,
                                risk_level,
                                reason,
                            };
                            // NeedsConfirmation is not an error — count as success
                            // since the tool correctly identified a dangerous action.
                            metrics::counter!(
                                "sober_agent_tool_calls_total",
                                "tool" => tool_name.clone(),
                                "status" => "success",
                            )
                            .increment(1);
                            Self::handle_confirmation(
                                &tool,
                                tool_input,
                                detail,
                                event_tx,
                                broadcast_tx,
                                conv_id_str,
                                registrar,
                                user_id,
                                repos,
                            )
                            .await
                        }
                        Err(e) => {
                            any_errors = true;
                            metrics::counter!(
                                "sober_agent_tool_calls_total",
                                "tool" => tool_name.clone(),
                                "status" => "error",
                            )
                            .increment(1);
                            format!("Tool error: {e}")
                        }
                    }
                }
                None => {
                    any_errors = true;
                    metrics::counter!(
                        "sober_agent_tool_calls_total",
                        "tool" => tool_name.clone(),
                        "status" => "not_found",
                    )
                    .increment(1);
                    format!("Tool not found: {tool_name}")
                }
            };
            metrics::histogram!(
                "sober_agent_tool_call_duration_seconds",
                "tool" => tool_name.clone(),
            )
            .record(tool_start.elapsed().as_secs_f64());

            let _ = event_tx
                .send(Ok(AgentEvent::ToolCallResult {
                    name: tool_name.clone(),
                    output: output.clone(),
                }))
                .await;
            let _ = broadcast_tx.send(proto::ConversationUpdate {
                conversation_id: conv_id_str.to_owned(),
                event: Some(proto::conversation_update::Event::ToolCallResult(
                    proto::ToolCallResult {
                        name: tool_name.clone(),
                        output: output.clone(),
                        internal: tool_internal,
                    },
                )),
            });

            messages.push(LlmMessage::tool(&tc.id, output.clone()));

            // Store tool result message for audit trail (skip internal tools).
            if !tool_internal
                && let Err(e) = repos
                    .messages()
                    .create(CreateMessage {
                        conversation_id,
                        role: MessageRole::Tool,
                        content: output.clone(),
                        tool_calls: None,
                        tool_result: Some(serde_json::json!({
                            "tool_call_id": tc.id,
                            "name": tool_name,
                        })),
                        token_count: None,
                        metadata: None,
                        user_id: Some(user_id),
                    })
                    .await
            {
                warn!(
                    tool = %tool_name,
                    error = %e,
                    "failed to persist tool result message"
                );
            }

            // Audit logging for shell tool runs.
            if let Some(ref cmd) = shell_command {
                let _ = crate::audit::log_shell_exec(
                    repos.audit_log(),
                    user_id,
                    workspace_id,
                    serde_json::json!({
                        "command": cmd,
                        "conversation_id": conversation_id.to_string(),
                    }),
                )
                .await;
            }
        }

        (messages, any_context_modifying, any_errors)
    }

    /// Handles the confirmation flow for a tool that returned `NeedsConfirmation`.
    ///
    /// Sends the confirmation event to the client, waits for the user's
    /// response, and re-executes the tool if approved.
    #[allow(clippy::too_many_arguments)]
    async fn handle_confirmation(
        tool: &Arc<dyn sober_core::types::tool::Tool>,
        mut tool_input: serde_json::Value,
        confirm: ConfirmDetail,
        event_tx: &mpsc::Sender<Result<AgentEvent, AgentError>>,
        broadcast_tx: &ConversationUpdateSender,
        conv_id_str: &str,
        registrar: Option<&ConfirmationRegistrar>,
        user_id: UserId,
        repos: &Arc<R>,
    ) -> String {
        let Some(registrar) = registrar else {
            return format!(
                "Command requires confirmation but no confirmation handler is available. \
                 Risk level: {}",
                confirm.risk_level
            );
        };

        // Register before sending the event so the broker is ready.
        let rx = registrar.register(confirm.confirm_id.clone());

        let _ = event_tx
            .send(Ok(AgentEvent::ConfirmRequest {
                confirm_id: confirm.confirm_id.clone(),
                command: confirm.command.clone(),
                risk_level: confirm.risk_level.clone(),
                affects: Vec::new(),
                reason: confirm.reason.clone(),
            }))
            .await;
        let _ = broadcast_tx.send(proto::ConversationUpdate {
            conversation_id: conv_id_str.to_owned(),
            event: Some(proto::conversation_update::Event::ConfirmRequest(
                proto::ConfirmRequest {
                    confirm_id: confirm.confirm_id,
                    command: confirm.command.clone(),
                    risk_level: confirm.risk_level.clone(),
                    affects: Vec::new(),
                    reason: confirm.reason.clone(),
                },
            )),
        });

        match tokio::time::timeout(std::time::Duration::from_secs(CONFIRM_TIMEOUT_SECS), rx).await {
            Ok(Ok(approved)) => {
                // Log the confirmation decision.
                let _ = crate::audit::log_confirmation(
                    repos.audit_log(),
                    user_id,
                    approved,
                    serde_json::json!({
                        "command": confirm.command,
                        "risk_level": confirm.risk_level,
                        "conversation_id": conv_id_str,
                    }),
                )
                .await;

                if approved {
                    // Re-execute with confirmed flag.
                    if let Some(obj) = tool_input.as_object_mut() {
                        obj.insert("confirmed".to_string(), serde_json::Value::Bool(true));
                    }
                    match tool.execute(tool_input).await {
                        Ok(output) => output.content,
                        Err(e) => format!("Tool error after confirmation: {e}"),
                    }
                } else {
                    "Command denied by user.".to_string()
                }
            }
            Ok(Err(_)) => "Confirmation cancelled.".to_string(),
            Err(_) => "Command timed out waiting for confirmation.".to_string(),
        }
    }

    /// Spawns a background task that embeds and stores inline memory extractions.
    ///
    /// Extractions are concise facts/preferences parsed from the LLM's
    /// `<memory_extractions>` block, replacing the old raw conversation ingestion.
    fn spawn_extraction_ingestion(
        llm: &Arc<dyn LlmEngine>,
        memory: &Arc<MemoryStore>,
        user_id: UserId,
        extractions: Vec<crate::extraction::MemoryExtraction>,
        half_life_days: u32,
    ) {
        let llm = Arc::clone(llm);
        let memory = Arc::clone(memory);
        let scope_id = sober_core::ScopeId::from_uuid(*user_id.as_uuid());

        tokio::spawn(async move {
            let decay_at = chrono::Utc::now() + chrono::Duration::days(half_life_days as i64);

            // Batch embed all extraction contents.
            let texts: Vec<&str> = extractions.iter().map(|e| e.content.as_str()).collect();
            let vectors = match llm.embed(&texts).await {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, "extraction ingestion: embedding failed, skipping");
                    return;
                }
            };

            if vectors.len() != extractions.len() {
                warn!(
                    "extraction ingestion: expected {} vectors, got {}",
                    extractions.len(),
                    vectors.len()
                );
                return;
            }

            for (extraction, dense_vector) in extractions.into_iter().zip(vectors) {
                let Some(chunk_type) =
                    crate::extraction::parse_extraction_type(&extraction.chunk_type)
                else {
                    debug!(
                        chunk_type = extraction.chunk_type,
                        "extraction ingestion: unknown chunk type, skipping"
                    );
                    continue;
                };

                let importance = crate::extraction::extraction_importance(chunk_type);

                if let Err(e) = memory
                    .store(
                        user_id,
                        StoreChunk {
                            dense_vector,
                            content: extraction.content,
                            chunk_type,
                            scope_id,
                            source_message_id: None,
                            importance,
                            decay_at,
                        },
                    )
                    .await
                {
                    warn!(error = %e, "extraction ingestion: failed to store");
                }
            }

            debug!("extraction ingestion complete for user {user_id}");
        });
    }
}

/// Converts domain [`DomainMessage`] values to LLM [`LlmMessage`] values.
pub fn domain_to_llm_messages(msgs: &[DomainMessage]) -> Vec<LlmMessage> {
    use std::collections::HashSet;

    let mut result: Vec<LlmMessage> = Vec::with_capacity(msgs.len());
    // Track which tool_call_ids are pending (from assistant messages).
    let mut pending_tool_ids: HashSet<String> = HashSet::new();

    for m in msgs {
        // Skip empty assistant messages without tool_calls.
        if m.role == MessageRole::Assistant && m.content.is_empty() && m.tool_calls.is_none() {
            continue;
        }
        // Skip event messages.
        if m.role == MessageRole::Event {
            continue;
        }

        let role = match m.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
            MessageRole::Event => unreachable!(),
        };

        // Restore tool_calls on assistant messages.
        let tool_calls: Option<Vec<LlmToolCall>> = if m.role == MessageRole::Assistant {
            m.tool_calls
                .as_ref()
                .and_then(|v| serde_json::from_value(v.clone()).ok())
        } else {
            None
        };

        // Track pending tool_call_ids from assistant messages.
        if let Some(ref tcs) = tool_calls {
            for tc in tcs {
                pending_tool_ids.insert(tc.id.clone());
            }
        }

        // When a non-tool message arrives and there are still pending tool_call_ids,
        // the sequence is broken (e.g., user interrupted). Drop the pending assistant
        // message's tool_calls by replacing the last assistant message.
        if m.role == MessageRole::User && !pending_tool_ids.is_empty() {
            // The sequence was broken by a user message. Remove the preceding
            // assistant message if it only existed for its tool_calls.
            if let Some(last) = result.last()
                && last.role == "assistant"
                && last.tool_calls.is_some()
            {
                let empty_content = last.content.as_ref().is_none_or(|c| c.trim().is_empty());
                if empty_content {
                    result.pop();
                } else {
                    let last_mut = result.last_mut().unwrap();
                    last_mut.tool_calls = None;
                }
            }
            pending_tool_ids.clear();
        }

        // Restore tool_call_id on tool messages.
        let tool_call_id = if m.role == MessageRole::Tool {
            m.tool_result
                .as_ref()
                .and_then(|v| v.get("tool_call_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned())
        } else {
            None
        };

        // Skip orphaned tool messages (no matching pending tool_call_id).
        if m.role == MessageRole::Tool {
            match &tool_call_id {
                Some(id) if pending_tool_ids.remove(id) => {
                    // Matched — keep this tool message.
                }
                _ => {
                    // Orphaned tool message — skip it.
                    continue;
                }
            }
        }

        // Restore reasoning_content from metadata if present (persisted during tool-call storage).
        let reasoning_content = m
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("reasoning_content"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned());

        result.push(LlmMessage {
            role: role.to_owned(),
            content: Some(m.content.clone()),
            reasoning_content,
            tool_calls,
            tool_call_id,
        });
    }

    result
}

/// Formats loaded memory context into a human-readable string for inclusion
/// in the task context.
pub fn format_memory_context(context: &LoadedContext) -> String {
    let mut parts = Vec::new();

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
        assert_eq!(config.conversation_history_limit, 50);
        assert_eq!(config.hits_per_scope, 5);
        assert_eq!(config.model, "anthropic/claude-sonnet-4");
        assert_eq!(config.embedding_model, "text-embedding-3-small");
        assert_eq!(config.max_tokens, 4096);
    }

    #[test]
    fn domain_to_llm_message_conversion() {
        let domain_msgs = vec![
            DomainMessage {
                id: sober_core::MessageId::new(),
                conversation_id: sober_core::ConversationId::new(),
                role: MessageRole::System,
                content: "You are helpful.".to_owned(),
                tool_calls: None,
                tool_result: None,
                token_count: None,
                user_id: None,
                metadata: None,
                created_at: chrono::Utc::now(),
            },
            DomainMessage {
                id: sober_core::MessageId::new(),
                conversation_id: sober_core::ConversationId::new(),
                role: MessageRole::User,
                content: "Hello!".to_owned(),
                tool_calls: None,
                tool_result: None,
                token_count: None,
                user_id: None,
                metadata: None,
                created_at: chrono::Utc::now(),
            },
            DomainMessage {
                id: sober_core::MessageId::new(),
                conversation_id: sober_core::ConversationId::new(),
                role: MessageRole::Assistant,
                content: "Hi there.".to_owned(),
                tool_calls: None,
                tool_result: None,
                token_count: None,
                user_id: None,
                metadata: None,
                created_at: chrono::Utc::now(),
            },
            DomainMessage {
                id: sober_core::MessageId::new(),
                conversation_id: sober_core::ConversationId::new(),
                role: MessageRole::Tool,
                content: "result".to_owned(),
                tool_calls: None,
                tool_result: None,
                token_count: None,
                user_id: None,
                metadata: None,
                created_at: chrono::Utc::now(),
            },
        ];

        let llm_msgs = domain_to_llm_messages(&domain_msgs);
        // Tool message is orphaned (no preceding assistant tool_call), so it's dropped.
        assert_eq!(llm_msgs.len(), 3);
        assert_eq!(llm_msgs[0].role, "system");
        assert_eq!(llm_msgs[0].content.as_deref(), Some("You are helpful."));
        assert_eq!(llm_msgs[1].role, "user");
        assert_eq!(llm_msgs[1].content.as_deref(), Some("Hello!"));
        assert_eq!(llm_msgs[2].role, "assistant");
        assert_eq!(llm_msgs[2].content.as_deref(), Some("Hi there."));
    }

    #[test]
    fn format_memory_context_empty() {
        let context = LoadedContext {
            recent_messages: vec![],
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
