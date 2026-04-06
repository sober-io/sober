//! Per-conversation actor — processes messages sequentially through an inbox channel.
//!
//! Each active conversation is represented by a [`ConversationActor`] running as
//! an independent tokio task. Messages arrive through an [`InboxMessage`] channel
//! and are processed one at a time, ensuring strict ordering within a conversation.
//!
//! The actor:
//! - Recovers incomplete tool executions on startup (crash recovery).
//! - Resolves (or provisions) the workspace for the conversation.
//! - Stores user messages and delegates to [`turn::run_turn`] for the agentic loop.
//! - Shuts down after an idle timeout or an explicit [`InboxMessage::Shutdown`].

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sober_core::types::AgentRepos;
use sober_core::types::ContentBlock;
use sober_core::types::Conversation;
use sober_core::types::access::TriggerKind;
use sober_core::types::domain::WorkspaceSettings;
use sober_core::types::enums::{AgentMode, MessageRole, ToolExecutionStatus};
use sober_core::types::ids::{ConversationId, MessageId, UserId};
use sober_core::types::input::CreateMessage;
use sober_core::types::repo::{
    ConversationRepo, MessageRepo, ToolExecutionRepo, WorkspaceRepo, WorkspaceSettingsRepo,
};
use sober_mind::assembly::Mind;
use sober_mind::injection::InjectionVerdict;
use tokio::sync::mpsc;
use tracing::{debug, info, instrument, warn};

use crate::context::AgentContext;
use crate::error::AgentError;
use crate::grpc::proto;
use crate::stream::{AgentEvent, Usage};
use crate::tools::TurnContext;
use crate::turn::{self, TurnParams};

/// A message delivered to a [`ConversationActor`] through its inbox channel.
#[derive(Debug)]
pub enum InboxMessage {
    /// A user (or scheduler/replica) message to process through the agentic loop.
    UserMessage {
        /// The authenticated user sending the message.
        user_id: UserId,
        /// The message content blocks.
        content: Vec<ContentBlock>,
        /// What triggered this interaction (Human, Scheduler, Admin, Replica).
        trigger: TriggerKind,
        /// Origin identifier for the message (e.g. "web", "gateway", "cli").
        source: String,
        /// Channel for streaming [`AgentEvent`]s back to the caller.
        event_tx: mpsc::Sender<Result<AgentEvent, AgentError>>,
    },
    /// Graceful shutdown request — the actor finishes its current message and exits.
    Shutdown,
}

/// Mention trigger for the agent in group conversations.
const MENTION_TRIGGER: &str = "@sober";

/// A per-conversation actor that processes messages sequentially.
///
/// Spawned as a tokio task by the [`ActorRegistry`](crate::agent) when a
/// conversation receives its first message. The actor owns all dependencies
/// needed to construct [`TurnParams`] and delegates the actual agentic loop
/// to [`turn::run_turn`].
///
/// The actor exits when:
/// - It receives [`InboxMessage::Shutdown`].
/// - The inbox sender is dropped (all handles gone).
/// - It has been idle for [`IDLE_TIMEOUT`](Self::IDLE_TIMEOUT).
pub struct ConversationActor<R: AgentRepos> {
    /// The conversation this actor serves.
    conversation_id: ConversationId,
    /// Inbox channel receiver — messages arrive here sequentially.
    inbox: mpsc::Receiver<InboxMessage>,
    /// Shared agent dependencies.
    ctx: AgentContext<R>,
    /// Per-conversation skill activation tracking.
    skill_activations: Arc<std::sync::Mutex<sober_skill::SkillActivationState>>,
}

impl<R: AgentRepos> ConversationActor<R> {
    /// How long the actor waits for a new message before shutting down.
    const IDLE_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

    /// Creates a new conversation actor.
    ///
    /// The `inbox` receiver is the consuming end of the channel; the sender
    /// half is held by the [`ActorRegistry`] for dispatching messages.
    pub fn new(
        conversation_id: ConversationId,
        inbox: mpsc::Receiver<InboxMessage>,
        ctx: AgentContext<R>,
        skill_activations: Arc<std::sync::Mutex<sober_skill::SkillActivationState>>,
    ) -> Self {
        Self {
            conversation_id,
            inbox,
            ctx,
            skill_activations,
        }
    }

    /// Runs the actor's inbox loop until shutdown, channel close, or idle timeout.
    ///
    /// This method consumes the actor. The caller (typically [`ActorRegistry`])
    /// spawns it in a `tokio::spawn` and handles registry cleanup when the
    /// future completes.
    #[instrument(skip_all, fields(conversation.id = %self.conversation_id))]
    pub async fn run(mut self) {
        info!(
            conversation_id = %self.conversation_id,
            "conversation actor started"
        );

        // Crash recovery: mark incomplete tool executions as failed.
        self.recover_incomplete_executions().await;

        loop {
            match tokio::time::timeout(Self::IDLE_TIMEOUT, self.inbox.recv()).await {
                Ok(Some(InboxMessage::UserMessage {
                    user_id,
                    content,
                    trigger,
                    source,
                    event_tx,
                })) => {
                    self.handle_message(user_id, content, trigger, source, &event_tx)
                        .await;
                }
                Ok(Some(InboxMessage::Shutdown)) => {
                    debug!(
                        conversation_id = %self.conversation_id,
                        "conversation actor received shutdown"
                    );
                    break;
                }
                Ok(None) => {
                    debug!(
                        conversation_id = %self.conversation_id,
                        "conversation actor inbox closed"
                    );
                    break;
                }
                Err(_) => {
                    debug!(
                        conversation_id = %self.conversation_id,
                        idle_timeout_secs = Self::IDLE_TIMEOUT.as_secs(),
                        "conversation actor idle timeout"
                    );
                    break;
                }
            }
        }

        info!(
            conversation_id = %self.conversation_id,
            "conversation actor stopped"
        );
    }

    /// Processes a single user message through the full agentic pipeline.
    ///
    /// Steps:
    /// 1. Check for prompt injection.
    /// 2. Resolve (or provision) the workspace.
    /// 3. Store the user message (human triggers only).
    /// 4. Check agent mode (silent/mention filtering).
    /// 5. Build per-turn tools.
    /// 6. Load skill catalog.
    /// 7. Delegate to [`turn::run_turn`].
    /// 8. Report metrics and broadcast errors.
    async fn handle_message(
        &self,
        user_id: UserId,
        content: Vec<ContentBlock>,
        trigger: TriggerKind,
        source: String,
        event_tx: &mpsc::Sender<Result<AgentEvent, AgentError>>,
    ) {
        let request_start = Instant::now();
        let trigger_label = match trigger {
            TriggerKind::Human => "human",
            TriggerKind::Scheduler => "scheduler",
            TriggerKind::Replica => "replica",
            TriggerKind::Admin => "admin",
        };

        let result = self
            .handle_message_inner(user_id, &content, trigger, &source, event_tx)
            .await;

        let status_label = if result.is_ok() { "success" } else { "error" };
        metrics::counter!(
            "sober_agent_requests_total",
            "trigger" => trigger_label,
            "status" => status_label,
        )
        .increment(1);
        metrics::histogram!(
            "sober_agent_request_duration_seconds",
            "trigger" => trigger_label,
        )
        .record(request_start.elapsed().as_secs_f64());

        if let Err(ref e) = result {
            warn!(
                conversation_id = %self.conversation_id,
                error = %e,
                "turn failed"
            );
            let error_msg = e.to_string();
            let _ = event_tx.send(Err(result.unwrap_err())).await;
            let _ = self.ctx.broadcast_tx.send(proto::ConversationUpdate {
                conversation_id: self.conversation_id.to_string(),
                event: Some(proto::conversation_update::Event::Error(proto::Error {
                    message: error_msg,
                })),
            });
        }
    }

    /// Inner implementation of message handling — returns `Result` so the outer
    /// method can handle error reporting uniformly.
    #[instrument(skip(self, content, event_tx), fields(trigger = ?trigger))]
    async fn handle_message_inner(
        &self,
        user_id: UserId,
        content: &[ContentBlock],
        trigger: TriggerKind,
        source: &str,
        event_tx: &mpsc::Sender<Result<AgentEvent, AgentError>>,
    ) -> Result<(), AgentError> {
        // Extract text for injection checking and other text-based operations.
        let text_content = ContentBlock::extract_text(content);

        // 1. Injection check
        let verdict = Mind::check_injection(&text_content);
        match verdict {
            InjectionVerdict::Rejected { reason } => {
                warn!(verdict = "rejected", "input flagged as injection, aborting");
                return Err(AgentError::InjectionDetected(reason));
            }
            InjectionVerdict::Flagged { reason } => {
                warn!(
                    user_id = %user_id,
                    reason = %reason,
                    "input flagged for injection"
                );
            }
            InjectionVerdict::Pass => {}
        }

        // 2. Resolve conversation and workspace
        let mut conversation = self
            .ctx
            .repos
            .conversations()
            .get_by_id(self.conversation_id)
            .await
            .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;

        let (workspace_dir, workspace_settings) =
            self.ensure_workspace(&mut conversation, user_id).await;
        debug!(workspace_dir = ?workspace_dir, has_settings = workspace_settings.is_some(), "workspace resolved");

        // 3. Store user message (human-triggered only)
        let user_msg_id = if trigger == TriggerKind::Human {
            let user_msg = self
                .ctx
                .repos
                .messages()
                .create(CreateMessage {
                    conversation_id: self.conversation_id,
                    role: MessageRole::User,
                    content: content.to_vec(),
                    reasoning: None,
                    token_count: None,
                    metadata: None,
                    user_id: Some(user_id),
                })
                .await
                .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;
            // Broadcast the user message so all subscribers (API WebSocket,
            // gateway outbound loop) see it in real-time.
            let _ = self.ctx.broadcast_tx.send(proto::ConversationUpdate {
                conversation_id: self.conversation_id.to_string(),
                event: Some(proto::conversation_update::Event::NewMessage(
                    proto::NewMessage {
                        message_id: user_msg.id.to_string(),
                        role: proto::MessageRole::User.into(),
                        content: crate::grpc::content_blocks::domain_to_proto(content),
                        source: source.to_owned(),
                        user_id: Some(user_id.to_string()),
                    },
                )),
            });

            user_msg.id
        } else {
            MessageId::new()
        };

        // 4. Check agent mode — decide whether to run the LLM pipeline
        let should_respond = match conversation.agent_mode {
            AgentMode::Always => true,
            AgentMode::Silent => false,
            AgentMode::Mention => text_content.to_lowercase().contains(MENTION_TRIGGER),
        };
        debug!(mode = ?conversation.agent_mode, "agent mode resolved");

        if !should_respond {
            debug!(
                agent_mode = ?conversation.agent_mode,
                conversation_id = %self.conversation_id,
                "skipping LLM pipeline due to agent mode"
            );
            let _ = event_tx
                .send(Ok(AgentEvent::Done {
                    message_id: user_msg_id,
                    content: None,
                    usage: Usage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                    },
                    artifact_ref: None,
                }))
                .await;
            return Ok(());
        }

        // 5. Build per-turn tool registry
        let tool_registry = {
            let turn_ctx = TurnContext {
                user_id,
                conversation_id: self.conversation_id,
                workspace_id: conversation.workspace_id,
                workspace_dir: workspace_dir.clone(),
                workspace_settings: workspace_settings.clone(),
                skill_activation_state: Some(Arc::clone(&self.skill_activations)),
            };
            Arc::new(
                self.ctx
                    .tool_bootstrap
                    .build(&turn_ctx, &self.ctx.static_tools)
                    .await,
            )
        };
        debug!(tool_count = tool_registry.len(), "tool registry built");

        // 6. Load skill catalog XML for system prompt injection
        let skill_catalog_xml = {
            let user_home = sober_workspace::user_home_dir();
            let ws_path = workspace_dir.clone().unwrap_or_default();
            match self
                .ctx
                .tool_bootstrap
                .plugin_manager
                .skill_loader()
                .load(&user_home, &ws_path)
                .await
            {
                Ok(catalog) => catalog.to_catalog_xml(),
                Err(e) => {
                    warn!(error = %e, "failed to load skill catalog");
                    String::new()
                }
            }
        };

        // 7. Build TurnParams and run the agentic turn
        let params = TurnParams {
            ctx: &self.ctx,
            tool_registry: &tool_registry,
            user_id,
            conversation_id: self.conversation_id,
            content: &text_content,
            user_msg_id,
            event_tx,
            trigger,
            conversation_kind: conversation.kind,
            workspace_id: conversation.workspace_id,
            workspace_dir,
            skill_catalog_xml,
        };

        turn::run_turn(&params).await
    }

    /// Provisions a workspace for the conversation if one doesn't exist yet,
    /// resolves the workspace directory path, and loads workspace settings.
    #[instrument(skip(self, conversation))]
    async fn ensure_workspace(
        &self,
        conversation: &mut Conversation,
        user_id: UserId,
    ) -> (Option<PathBuf>, Option<WorkspaceSettings>) {
        let workspace_root = self.ctx.config.workspace_root.display().to_string();

        // Provision workspace + settings if the conversation doesn't have one yet
        // (fallback for pre-migration conversations).
        if conversation.workspace_id.is_none() {
            let root_path = format!("{}/{}", workspace_root, self.conversation_id);
            match self
                .ctx
                .repos
                .workspaces()
                .provision(user_id, &self.conversation_id.to_string(), &root_path)
                .await
            {
                Ok((ws, _settings)) => {
                    if let Err(e) = self
                        .ctx
                        .repos
                        .conversations()
                        .update_workspace(self.conversation_id, Some(ws.id))
                        .await
                    {
                        warn!("failed to link workspace to conversation: {e}");
                    } else {
                        conversation.workspace_id = Some(ws.id);
                    }
                }
                Err(e) => {
                    warn!("failed to provision workspace: {e}");
                }
            }
        }

        // Resolve workspace directory path.
        let ws_id = match conversation.workspace_id {
            Some(id) => id,
            None => return (None, None),
        };

        let ws = match self.ctx.repos.workspaces().get_by_id(ws_id).await {
            Ok(ws) => ws,
            Err(_) => return (None, None),
        };

        let root = PathBuf::from(&ws.root_path);
        let dir = match tokio::fs::create_dir_all(&root).await {
            Ok(()) => Some(root),
            Err(e) => {
                warn!(
                    workspace_id = %ws_id,
                    root_path = %ws.root_path,
                    "failed to create workspace dir: {e}"
                );
                None
            }
        };

        // Load workspace settings from DB.
        let settings = self
            .ctx
            .repos
            .workspace_settings()
            .get_by_workspace(ws_id)
            .await
            .ok();

        (dir, settings)
    }

    /// Marks incomplete (pending/running) tool executions as failed.
    ///
    /// Called once when the actor starts to recover from a previous crash.
    /// If the agent process was killed mid-execution, some tool executions
    /// may be stuck in `Pending` or `Running` status. This marks them as
    /// `Failed` so they don't block future context reconstruction.
    #[instrument(skip(self))]
    async fn recover_incomplete_executions(&self) {
        match self
            .ctx
            .repos
            .tool_executions()
            .find_incomplete(self.conversation_id)
            .await
        {
            Ok(incomplete) if incomplete.is_empty() => {}
            Ok(incomplete) => {
                let count = incomplete.len();
                info!(
                    conversation_id = %self.conversation_id,
                    count,
                    "recovering incomplete tool executions"
                );
                for exec in incomplete {
                    if let Err(e) = self
                        .ctx
                        .repos
                        .tool_executions()
                        .update_status(
                            exec.id,
                            ToolExecutionStatus::Failed,
                            None,
                            Some("Agent restarted during execution"),
                        )
                        .await
                    {
                        warn!(
                            execution_id = %exec.id,
                            error = %e,
                            "failed to recover tool execution"
                        );
                    }
                }
                info!(recovered = count, "recovered incomplete tool executions");
            }
            Err(e) => {
                warn!(
                    conversation_id = %self.conversation_id,
                    error = %e,
                    "failed to query incomplete tool executions"
                );
            }
        }
    }
}
