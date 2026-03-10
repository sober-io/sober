//! Core agent logic — the agentic loop that handles user messages.
//!
//! The [`Agent`] struct orchestrates the full message-handling flow:
//! injection detection → context loading → prompt assembly → LLM completion
//! → tool execution → response streaming.

use std::sync::Arc;

use sober_core::config::MemoryConfig;
use sober_core::types::access::{CallerContext, TriggerKind};
use sober_core::types::domain::Message as DomainMessage;
use sober_core::types::enums::MessageRole;
use sober_core::types::ids::{ConversationId, UserId};
use sober_core::types::input::CreateMessage;
use sober_core::types::repo::{ConversationRepo, McpServerRepo, MessageRepo};
use sober_llm::types::{CompletionRequest, ToolCall as LlmToolCall};
use sober_llm::{LlmEngine, Message as LlmMessage};
use sober_memory::{ContextLoader, LoadRequest, LoadedContext, MemoryStore};
use sober_mind::assembly::{Mind, TaskContext};
use sober_mind::injection::InjectionVerdict;
use tracing::{debug, warn};

use crate::error::AgentError;
use crate::stream::{AgentEvent, AgentResponseStream, Usage};
use crate::tools::ToolRegistry;

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
            max_tool_iterations: 10,
            context_token_budget: 128_000,
            conversation_history_limit: 50,
            hits_per_scope: 10,
            model: "anthropic/claude-sonnet-4".to_owned(),
            embedding_model: "text-embedding-3-small".to_owned(),
            max_tokens: 4096,
        }
    }
}

/// The core agent that handles messages through an agentic loop.
///
/// Generic over repository types so it can be tested without a real database.
pub struct Agent<Msg, Conv, Mcp>
where
    Msg: MessageRepo,
    Conv: ConversationRepo,
    Mcp: McpServerRepo,
{
    llm: Arc<dyn LlmEngine>,
    mind: Arc<Mind>,
    #[allow(dead_code)]
    memory: Arc<MemoryStore>,
    context_loader: Arc<ContextLoader<Msg>>,
    tool_registry: Arc<ToolRegistry>,
    message_repo: Arc<Msg>,
    #[allow(dead_code)]
    conversation_repo: Arc<Conv>,
    #[allow(dead_code)]
    mcp_server_repo: Arc<Mcp>,
    config: AgentConfig,
    memory_config: MemoryConfig,
}

impl<Msg, Conv, Mcp> Agent<Msg, Conv, Mcp>
where
    Msg: MessageRepo + 'static,
    Conv: ConversationRepo + 'static,
    Mcp: McpServerRepo + 'static,
{
    /// Creates a new agent with all required dependencies.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        llm: Arc<dyn LlmEngine>,
        mind: Arc<Mind>,
        memory: Arc<MemoryStore>,
        context_loader: Arc<ContextLoader<Msg>>,
        tool_registry: Arc<ToolRegistry>,
        message_repo: Arc<Msg>,
        conversation_repo: Arc<Conv>,
        mcp_server_repo: Arc<Mcp>,
        config: AgentConfig,
        memory_config: MemoryConfig,
    ) -> Self {
        Self {
            llm,
            mind,
            memory,
            context_loader,
            tool_registry,
            message_repo,
            conversation_repo,
            mcp_server_repo,
            config,
            memory_config,
        }
    }

    /// Handles a user message through the full agentic loop.
    ///
    /// Returns a stream of [`AgentEvent`]s. For v1 this is a non-streaming
    /// implementation that collects all events and returns them as a vec-backed
    /// stream.
    pub async fn handle_message(
        &self,
        user_id: UserId,
        conversation_id: ConversationId,
        content: &str,
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

        // 2. Store user message
        let _user_msg = self
            .message_repo
            .create(CreateMessage {
                conversation_id,
                role: MessageRole::User,
                content: content.to_owned(),
                tool_calls: None,
                tool_result: None,
                token_count: None,
            })
            .await
            .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;

        // 3. Run the agentic loop
        let events = self.run_loop(user_id, conversation_id, content).await?;

        Ok(Box::pin(futures::stream::iter(events.into_iter().map(Ok))))
    }

    /// Inner agentic loop: embed → load context → assemble → complete → handle tool calls.
    ///
    /// On the first iteration (and after any context-modifying tool), the full
    /// embed → load → assemble pipeline runs. On subsequent non-rebuild iterations
    /// the cached `llm_messages` are reused with tool results appended, avoiding
    /// redundant embedding, context loading, and prompt assembly.
    async fn run_loop(
        &self,
        user_id: UserId,
        conversation_id: ConversationId,
        content: &str,
    ) -> Result<Vec<AgentEvent>, AgentError> {
        let mut events = Vec::new();
        let mut llm_messages: Vec<LlmMessage> = Vec::new();
        let mut needs_rebuild = true;

        for iteration in 0..self.config.max_tool_iterations {
            debug!(iteration, "agent loop iteration");

            if needs_rebuild {
                // a. Embed user message for context retrieval (soft-fail: proceed
                //    without memory context if embeddings are unavailable).
                let query_vector = match self.llm.embed(&[content]).await {
                    Ok(vecs) => vecs.into_iter().next().unwrap_or_default(),
                    Err(e) => {
                        warn!(error = %e, "embedding failed, proceeding without memory context");
                        vec![]
                    }
                };

                // b. Load context (skip memory retrieval when no embedding)
                let loaded_context = if query_vector.is_empty() {
                    // Still load recent messages even without embeddings.
                    self.context_loader
                        .load(
                            LoadRequest {
                                query_vector: vec![],
                                query_text: content.to_owned(),
                                user_id,
                                conversation_id,
                                token_budget: self.config.context_token_budget,
                                recent_message_count: self.config.conversation_history_limit,
                                hits_per_scope: 0,
                            },
                            &self.memory_config,
                        )
                        .await
                        .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?
                } else {
                    self.context_loader
                        .load(
                            LoadRequest {
                                query_vector,
                                query_text: content.to_owned(),
                                user_id,
                                conversation_id,
                                token_budget: self.config.context_token_budget,
                                recent_message_count: self.config.conversation_history_limit,
                                hits_per_scope: self.config.hits_per_scope,
                            },
                            &self.memory_config,
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
                };

                let memory_context_text = format_memory_context(&loaded_context);
                let task_context = TaskContext {
                    description: if memory_context_text.is_empty() {
                        String::new()
                    } else {
                        format!("## Relevant Memories\n\n{memory_context_text}")
                    },
                    recent_messages: loaded_context.recent_messages.clone(),
                };

                let tool_metadata = self.tool_registry.tool_metadata();
                let assembled = self
                    .mind
                    .assemble(&caller, &task_context, &tool_metadata, "")
                    .await
                    .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;

                // d. Convert domain messages to LLM messages (replaces cached messages)
                llm_messages = domain_to_llm_messages(&assembled);
                needs_rebuild = false;
            }

            // e. Call LLM with the (possibly cached) messages
            let tool_definitions = self.tool_registry.tool_definitions();
            let req = CompletionRequest {
                model: self.config.model.clone(),
                messages: llm_messages.clone(),
                tools: tool_definitions,
                max_tokens: Some(self.config.max_tokens),
                temperature: None,
                stop: vec![],
                stream: false,
            };

            let response = self
                .llm
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
                // Append the assistant's tool-call message to the cached history,
                // preserving reasoning_content so providers like Kimi don't reject it.
                llm_messages.push(LlmMessage {
                    role: "assistant".to_owned(),
                    content: choice.message.content.clone(),
                    reasoning_content: choice.message.reasoning_content.clone(),
                    tool_calls: choice.message.tool_calls.clone(),
                    tool_call_id: None,
                });

                let (tool_events, tool_msgs, any_context_modifying) =
                    self.execute_tool_calls(tool_calls).await;
                events.extend(tool_events);

                // Append tool result messages to cached history
                llm_messages.extend(tool_msgs);

                if any_context_modifying {
                    needs_rebuild = true;
                }
                continue;
            }

            // g. Text response — emit events and store assistant message
            let text = choice.message.content.unwrap_or_default();
            if !text.is_empty() {
                events.push(AgentEvent::TextDelta(text.clone()));
            }

            let assistant_msg = self
                .message_repo
                .create(CreateMessage {
                    conversation_id,
                    role: MessageRole::Assistant,
                    content: text,
                    tool_calls: None,
                    tool_result: None,
                    token_count: usage_stats.map(|u| u.total_tokens as i32),
                })
                .await
                .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;

            events.push(AgentEvent::Done {
                message_id: assistant_msg.id,
                usage: Usage {
                    prompt_tokens: usage_stats.map_or(0, |u| u.prompt_tokens),
                    completion_tokens: usage_stats.map_or(0, |u| u.completion_tokens),
                },
            });

            return Ok(events);
        }

        Err(AgentError::MaxIterationsExceeded(
            self.config.max_tool_iterations,
        ))
    }

    /// Executes a set of tool calls and returns events, LLM messages, and whether
    /// any tool was context-modifying.
    async fn execute_tool_calls(
        &self,
        tool_calls: &[LlmToolCall],
    ) -> (Vec<AgentEvent>, Vec<LlmMessage>, bool) {
        let mut events = Vec::new();
        let mut messages = Vec::new();
        let mut any_context_modifying = false;

        for tc in tool_calls {
            let tool_name = &tc.function.name;
            let tool_input: serde_json::Value = match serde_json::from_str(&tc.function.arguments) {
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

            events.push(AgentEvent::ToolCallStart {
                name: tool_name.clone(),
                input: tool_input.clone(),
            });

            let output = match self.tool_registry.get_tool(tool_name) {
                Some(tool) => {
                    // Check if context-modifying
                    if tool.metadata().context_modifying {
                        any_context_modifying = true;
                    }
                    match tool.execute(tool_input).await {
                        Ok(output) => output.content,
                        Err(e) => format!("Tool error: {e}"),
                    }
                }
                None => format!("Tool not found: {tool_name}"),
            };

            events.push(AgentEvent::ToolCallResult {
                name: tool_name.clone(),
                output: output.clone(),
            });

            messages.push(LlmMessage::tool(&tc.id, output));
        }

        (events, messages, any_context_modifying)
    }
}

/// Converts domain [`DomainMessage`] values to LLM [`LlmMessage`] values.
pub fn domain_to_llm_messages(msgs: &[DomainMessage]) -> Vec<LlmMessage> {
    msgs.iter()
        .map(|m| {
            let role = match m.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
            };
            LlmMessage {
                role: role.to_owned(),
                content: Some(m.content.clone()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            }
        })
        .collect()
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
        assert_eq!(config.max_tool_iterations, 10);
        assert_eq!(config.context_token_budget, 128_000);
        assert_eq!(config.conversation_history_limit, 50);
        assert_eq!(config.hits_per_scope, 10);
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
                created_at: chrono::Utc::now(),
            },
        ];

        let llm_msgs = domain_to_llm_messages(&domain_msgs);
        assert_eq!(llm_msgs.len(), 4);
        assert_eq!(llm_msgs[0].role, "system");
        assert_eq!(llm_msgs[0].content.as_deref(), Some("You are helpful."));
        assert_eq!(llm_msgs[1].role, "user");
        assert_eq!(llm_msgs[1].content.as_deref(), Some("Hello!"));
        assert_eq!(llm_msgs[2].role, "assistant");
        assert_eq!(llm_msgs[2].content.as_deref(), Some("Hi there."));
        assert_eq!(llm_msgs[3].role, "tool");
        assert_eq!(llm_msgs[3].content.as_deref(), Some("result"));
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
