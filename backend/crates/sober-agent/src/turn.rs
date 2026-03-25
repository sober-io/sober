//! Single turn of the agentic loop — streaming LLM call with tool dispatch.
//!
//! [`run_turn`] replaces the inner loop of `Agent::run_loop_streaming` with:
//!
//! - **Streaming LLM calls** (`llm.stream()` instead of `llm.complete()`)
//!   forwarding `TextDelta` events in real-time.
//! - **Write-ahead persistence** — the assistant message is stored *before*
//!   tool executions so that [`ToolExecution`] rows can FK to it.
//! - **`dispatch::execute_tool_calls`** for tool lifecycle management.
//! - **`history::to_llm_messages`** for converting persisted messages (with
//!   their tool executions) into the LLM message format.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt;
use sober_core::config::{LlmConfig, MemoryConfig};
use sober_core::types::AgentRepos;
use sober_core::types::access::{CallerContext, TriggerKind};
use sober_core::types::enums::{ConversationKind, MessageRole};
use sober_core::types::ids::{ConversationId, MessageId, UserId, WorkspaceId};
use sober_core::types::input::CreateMessage;
use sober_core::types::repo::{ConversationRepo, MessageRepo, ToolExecutionRepo, UserRepo};
use sober_crypto::envelope::Mek;
use sober_llm::types::{CompletionRequest, ToolCall as LlmToolCall};
use sober_llm::{LlmEngine, Message as LlmMessage, OpenAiCompatibleEngine};
use sober_memory::{ContextLoader, LoadRequest, MemoryStore};
use sober_mind::assembly::{Mind, TaskContext};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::agent::{AgentConfig, format_memory_context};
use crate::broadcast::ConversationUpdateSender;
use crate::confirm::ConfirmationRegistrar;
use crate::dispatch::{self, DispatchOutcome};
use crate::error::AgentError;
use crate::grpc::proto;
use crate::history;
use crate::stream::{AgentEvent, Usage};
use crate::tools::ToolRegistry;

/// Maximum consecutive tool errors before forcing a text response.
const MAX_CONSECUTIVE_TOOL_ERRORS: u32 = 3;

/// All parameters needed to execute a single agentic turn (multi-iteration loop).
///
/// This replaces the old `LoopContext` with individually-typed fields so that
/// `run_turn` can be called as a free function rather than an `Agent` method.
#[allow(clippy::too_many_arguments)]
pub struct TurnParams<'a, R: AgentRepos> {
    pub llm: &'a Arc<dyn LlmEngine>,
    pub mind: &'a Arc<Mind>,
    pub memory: &'a Arc<MemoryStore>,
    pub context_loader: &'a Arc<ContextLoader<R::Msg>>,
    pub tool_registry: &'a Arc<ToolRegistry>,
    pub repos: &'a Arc<R>,
    pub config: &'a AgentConfig,
    pub memory_config: &'a MemoryConfig,
    pub user_id: UserId,
    pub conversation_id: ConversationId,
    pub content: &'a str,
    pub user_msg_id: MessageId,
    pub event_tx: &'a mpsc::Sender<Result<AgentEvent, AgentError>>,
    pub broadcast_tx: &'a ConversationUpdateSender,
    pub registrar: Option<&'a ConfirmationRegistrar>,
    pub trigger: TriggerKind,
    pub conversation_kind: ConversationKind,
    pub workspace_id: Option<WorkspaceId>,
    pub workspace_dir: Option<PathBuf>,
    pub llm_config: &'a Option<LlmConfig>,
    pub mek: &'a Option<Arc<Mek>>,
    pub skill_catalog_xml: String,
}

/// Runs the full agentic turn: context load → stream LLM → tool dispatch → repeat.
///
/// Sends [`AgentEvent`]s through `event_tx` and broadcasts [`ConversationUpdate`]s
/// through `broadcast_tx` in real-time as the LLM streams its response.
///
/// # Errors
///
/// Returns [`AgentError`] if context loading, LLM streaming, or persistence fails
/// fatally. Tool execution errors are handled internally (fed back to the LLM).
pub async fn run_turn<R: AgentRepos>(params: &TurnParams<'_, R>) -> Result<(), AgentError> {
    let conv_id_str = params.conversation_id.to_string();
    let mut llm_messages: Vec<LlmMessage> = Vec::new();
    let mut needs_context_rebuild = true;
    let mut system_prompt_len: usize = 0;
    let mut consecutive_tool_errors: u32 = 0;
    let mut assistant_text = String::new();
    // Track the assistant message across iterations so tool-call and text
    // responses end up in a single DB row.
    let mut turn_assistant_msg_id: Option<MessageId> = None;

    for iteration in 0..params.config.max_tool_iterations {
        debug!(iteration, "agent turn iteration");
        metrics::counter!("sober_agent_loop_iterations_total").increment(1);

        // -----------------------------------------------------------------
        // Phase 1: Context loading and prompt assembly
        // -----------------------------------------------------------------
        if needs_context_rebuild {
            let (system_prompt, history_messages) = build_context(params, &conv_id_str).await?;

            // Preserve in-memory tool history from previous iterations so
            // the LLM doesn't re-execute already-completed tool calls after
            // a context rebuild.
            let tool_history: Vec<LlmMessage> =
                if system_prompt_len > 0 && llm_messages.len() > system_prompt_len {
                    llm_messages.split_off(system_prompt_len)
                } else {
                    Vec::new()
                };

            // Build the message list: system prompt + conversation history
            llm_messages = Vec::with_capacity(1 + history_messages.len() + tool_history.len());
            llm_messages.push(LlmMessage::system(system_prompt));
            llm_messages.extend(history_messages);

            system_prompt_len = llm_messages.len();

            // For non-human triggers the prompt wasn't stored in the DB,
            // so the context loader didn't include it. Add it explicitly.
            if params.trigger != TriggerKind::Human {
                llm_messages.push(LlmMessage::user(params.content));
            }

            llm_messages.extend(tool_history);
            needs_context_rebuild = false;
        }

        // -----------------------------------------------------------------
        // Phase 2: Streaming LLM call
        // -----------------------------------------------------------------
        let tool_definitions = if params.trigger == TriggerKind::Scheduler {
            params.tool_registry.tool_definitions_except(&["scheduler"])
        } else {
            params.tool_registry.tool_definitions()
        };

        let req = CompletionRequest {
            model: params.config.model.clone(),
            messages: llm_messages.clone(),
            tools: tool_definitions,
            max_tokens: Some(params.config.max_tokens),
            temperature: None,
            stop: vec![],
            stream: true,
        };

        // Try dynamic key resolution for user-stored LLM credentials.
        let dynamic_engine: Option<OpenAiCompatibleEngine> = try_resolve_dynamic_engine(
            params.repos,
            params.user_id,
            params.conversation_id,
            params.llm_config,
            params.mek,
        )
        .await;
        let effective_llm: &dyn LlmEngine = match dynamic_engine {
            Some(ref engine) => engine,
            None => &**params.llm,
        };

        let mut stream = effective_llm
            .stream(req)
            .await
            .map_err(|e| AgentError::LlmCallFailed(e.to_string()))?;

        let mut content_buffer = String::new();
        let mut reasoning_buffer = String::new();
        let mut tool_call_buffers: HashMap<u32, (String, String, String)> = HashMap::new();
        let mut usage_stats: Option<sober_llm::types::Usage> = None;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| AgentError::LlmCallFailed(e.to_string()))?;

            if chunk.usage.is_some() {
                usage_stats = chunk.usage;
            }

            for choice in &chunk.choices {
                // Text content delta — forward immediately.
                if let Some(ref text) = choice.delta.content {
                    content_buffer.push_str(text);
                    let _ = params
                        .event_tx
                        .send(Ok(AgentEvent::TextDelta(text.clone())))
                        .await;
                    let _ = params.broadcast_tx.send(proto::ConversationUpdate {
                        conversation_id: conv_id_str.clone(),
                        event: Some(proto::conversation_update::Event::TextDelta(
                            proto::TextDelta {
                                content: text.clone(),
                            },
                        )),
                    });
                }

                // Reasoning content delta — accumulate silently.
                if let Some(ref reasoning) = choice.delta.reasoning_content {
                    reasoning_buffer.push_str(reasoning);
                }

                // Tool call deltas — buffer until stream ends.
                if let Some(ref tool_calls) = choice.delta.tool_calls {
                    for tc_delta in tool_calls {
                        let entry = tool_call_buffers
                            .entry(tc_delta.index)
                            .or_insert_with(|| (String::new(), String::new(), String::new()));
                        if let Some(ref id) = tc_delta.id {
                            entry.0.clone_from(id);
                        }
                        if let Some(ref func) = tc_delta.function {
                            if let Some(ref name) = func.name {
                                entry.1.clone_from(name);
                            }
                            if let Some(ref args) = func.arguments {
                                entry.2.push_str(args);
                            }
                        }
                    }
                }
            }
        }

        // Assemble tool calls from buffered deltas.
        let tool_calls: Vec<LlmToolCall> = {
            let mut entries: Vec<_> = tool_call_buffers.into_iter().collect();
            entries.sort_by_key(|(idx, _)| *idx);
            entries
                .into_iter()
                .map(|(_, (id, name, args))| LlmToolCall {
                    id,
                    r#type: "function".to_owned(),
                    function: sober_llm::types::FunctionCall {
                        name,
                        arguments: args,
                    },
                })
                .collect()
        };
        let has_tool_calls = !tool_calls.is_empty();

        // -----------------------------------------------------------------
        // Phase 3: Handle tool calls
        // -----------------------------------------------------------------
        if has_tool_calls {
            // Add assistant message with tool calls to the in-memory LLM
            // message list so future iterations see the full history.
            llm_messages.push(LlmMessage {
                role: "assistant".to_owned(),
                content: if content_buffer.is_empty() {
                    None
                } else {
                    Some(content_buffer.clone())
                },
                reasoning_content: if reasoning_buffer.is_empty() {
                    None
                } else {
                    Some(reasoning_buffer.clone())
                },
                tool_calls: Some(tool_calls.clone()),
                tool_call_id: None,
            });

            // Skip DB persistence when all tool calls target internal tools
            // (e.g. activate_skill) — they stay in-memory only.
            let all_internal = tool_calls.iter().all(|tc| {
                params
                    .tool_registry
                    .get_tool(&tc.function.name)
                    .is_some_and(|t| t.metadata().internal)
            });

            // Write-ahead: create the assistant message on first tool-call
            // iteration, reuse on subsequent iterations. This ensures all tool
            // executions and the final text live on one message row.
            let assistant_msg_id = if !all_internal {
                if let Some(existing_id) = turn_assistant_msg_id {
                    existing_id
                } else {
                    let assistant_msg = params
                        .repos
                        .messages()
                        .create(CreateMessage {
                            conversation_id: params.conversation_id,
                            role: MessageRole::Assistant,
                            content: content_buffer.clone(),
                            reasoning: if reasoning_buffer.is_empty() {
                                None
                            } else {
                                Some(reasoning_buffer.clone())
                            },
                            token_count: usage_stats.map(|u| u.total_tokens as i32),
                            metadata: None,
                            user_id: Some(params.user_id),
                        })
                        .await
                        .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;
                    turn_assistant_msg_id = Some(assistant_msg.id);
                    assistant_msg.id
                }
            } else {
                // Internal-only tool calls: use a temporary ID (not persisted).
                MessageId::new()
            };

            // Dispatch tool calls via the write-ahead dispatch module.
            let outcome: DispatchOutcome = dispatch::execute_tool_calls(
                &tool_calls,
                assistant_msg_id,
                params.conversation_id,
                params.repos.as_ref(),
                params.tool_registry,
                params.event_tx,
                params.broadcast_tx,
                &conv_id_str,
                params.registrar,
                params.user_id,
                params.workspace_id,
            )
            .await;

            // Append tool result messages to in-memory LLM messages.
            for result in &outcome.results {
                llm_messages.push(LlmMessage::tool(&result.tool_call_id, &result.output));
            }

            if outcome.any_context_modifying {
                needs_context_rebuild = true;
            }

            if outcome.any_errors {
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

        // -----------------------------------------------------------------
        // Phase 4: Text response — no tool calls
        // -----------------------------------------------------------------

        // Strip memory extractions before storing/broadcasting.
        let extraction_result = crate::extraction::strip_extractions(&content_buffer);
        let text = extraction_result.cleaned_text;

        if !text.is_empty() {
            // Note: text was already streamed via TextDelta events above.
            // We still need to send a NewMessage event with the final cleaned
            // text after extraction stripping for the DB record.
            assistant_text = text.clone();
        }

        // Store the assistant response. If tool calls ran earlier in this
        // turn, update the existing message; otherwise create a new one.
        let assistant_msg_id = if let Some(existing_id) = turn_assistant_msg_id {
            // Update the message that was created during tool-call phase.
            params
                .repos
                .messages()
                .update_content(
                    existing_id,
                    &text,
                    if reasoning_buffer.is_empty() {
                        None
                    } else {
                        Some(&reasoning_buffer)
                    },
                )
                .await
                .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;
            existing_id
        } else {
            let assistant_msg = params
                .repos
                .messages()
                .create(CreateMessage {
                    conversation_id: params.conversation_id,
                    role: MessageRole::Assistant,
                    content: text.clone(),
                    reasoning: if reasoning_buffer.is_empty() {
                        None
                    } else {
                        Some(reasoning_buffer.clone())
                    },
                    token_count: usage_stats.map(|u| u.total_tokens as i32),
                    metadata: None,
                    user_id: Some(params.user_id),
                })
                .await
                .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;
            assistant_msg.id
        };

        // Spawn background extraction ingestion.
        if !extraction_result.extractions.is_empty() {
            crate::ingestion::spawn_extraction_ingestion(
                params.llm,
                params.memory,
                params.user_id,
                extraction_result.extractions,
                params.memory_config.decay_half_life_days,
            );
        }

        // Broadcast NewMessage for the assistant message.
        let _ = params.broadcast_tx.send(proto::ConversationUpdate {
            conversation_id: conv_id_str.clone(),
            event: Some(proto::conversation_update::Event::NewMessage(
                proto::NewMessage {
                    message_id: assistant_msg_id.to_string(),
                    role: "Assistant".to_owned(),
                    content: text.clone(),
                    source: format!("{:?}", params.trigger).to_lowercase(),
                    user_id: Some(params.user_id.to_string()),
                },
            )),
        });

        // Send Done event.
        let _ = params
            .event_tx
            .send(Ok(AgentEvent::Done {
                message_id: assistant_msg_id,
                usage: Usage {
                    prompt_tokens: usage_stats.map_or(0, |u| u.prompt_tokens),
                    completion_tokens: usage_stats.map_or(0, |u| u.completion_tokens),
                },
                artifact_ref: None,
            }))
            .await;
        let _ = params.broadcast_tx.send(proto::ConversationUpdate {
            conversation_id: conv_id_str.clone(),
            event: Some(proto::conversation_update::Event::Done(proto::Done {
                message_id: assistant_msg_id.to_string(),
                prompt_tokens: usage_stats.map_or(0, |u| u.prompt_tokens),
                completion_tokens: usage_stats.map_or(0, |u| u.completion_tokens),
                artifact_ref: String::new(),
            })),
        });

        // -----------------------------------------------------------------
        // Phase 5: Auto-generate title
        // -----------------------------------------------------------------
        auto_generate_title(params, &conv_id_str, &assistant_text).await;

        metrics::histogram!("sober_agent_loop_iterations_per_request")
            .record(f64::from(iteration + 1));
        return Ok(());
    }

    metrics::histogram!("sober_agent_loop_iterations_per_request")
        .record(f64::from(params.config.max_tool_iterations));
    Err(AgentError::MaxIterationsExceeded(
        params.config.max_tool_iterations,
    ))
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Builds the system prompt and conversation history messages for an LLM call.
///
/// Returns `(system_prompt, history_messages)` where:
/// - `system_prompt` is the assembled system prompt from Mind
/// - `history_messages` are the conversation messages in LLM format, built
///   from persisted messages + tool executions via `history::to_llm_messages`
async fn build_context<R: AgentRepos>(
    params: &TurnParams<'_, R>,
    _conv_id_str: &str,
) -> Result<(String, Vec<LlmMessage>), AgentError> {
    // a. Embed user message for context retrieval
    let query_vector = match params.llm.embed(&[params.content]).await {
        Ok(vecs) => vecs.into_iter().next().unwrap_or_default(),
        Err(e) => {
            warn!(error = %e, "embedding failed, proceeding without memory context");
            vec![]
        }
    };

    // b. Load context
    let recent_message_count = if params.trigger == TriggerKind::Human {
        params.config.conversation_history_limit
    } else {
        0
    };
    let hits_per_scope = if query_vector.is_empty() {
        0
    } else {
        params.config.hits_per_scope
    };

    let loaded_context = params
        .context_loader
        .load(
            LoadRequest {
                query_vector,
                query_text: params.content.to_owned(),
                user_id: params.user_id,
                conversation_id: params.conversation_id,
                token_budget: params.config.context_token_budget,
                recent_message_count,
                hits_per_scope,
            },
            params.memory_config,
        )
        .await
        .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;

    // c. Assemble prompt via Mind
    let caller = CallerContext {
        user_id: Some(params.user_id),
        trigger: params.trigger,
        permissions: vec![],
        scope_grants: vec![],
        workspace_id: None,
    };

    // Build user display names for group conversations.
    let user_display_names = if params.conversation_kind == ConversationKind::Group {
        let user_ids: std::collections::HashSet<UserId> = loaded_context
            .recent_messages
            .iter()
            .filter_map(|m| m.user_id)
            .collect();
        let mut names = HashMap::new();
        for uid in user_ids {
            if let Ok(user) = params.repos.users().get_by_id(uid).await {
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
    if let Some(ref ws_dir) = params.workspace_dir {
        task_description.push_str(&format!(
            "## Workspace\n\nCurrent workspace directory: `{}`\n\n",
            ws_dir.display()
        ));
    }

    if !memory_context_text.is_empty() {
        task_description.push_str(&format!("## Relevant Memories\n\n{memory_context_text}"));
    }

    let task_context = TaskContext {
        description: task_description,
        recent_messages: loaded_context.recent_messages.clone(),
        conversation_kind: params.conversation_kind,
        user_display_names,
    };

    let tool_metadata = params.tool_registry.tool_metadata();
    let assembled = params
        .mind
        .assemble(
            &caller,
            &task_context,
            &tool_metadata,
            "",
            &params.skill_catalog_xml,
        )
        .await
        .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;

    // d. Build system prompt from the first system message in assembled output.
    //    The Mind's assemble() returns Vec<DomainMessage> where the first entry
    //    is typically the system prompt. Extract it and use history::to_llm_messages
    //    for the conversation history from DB.
    let system_prompt = assembled
        .iter()
        .find(|m| m.role == MessageRole::System)
        .map(|m| m.content.clone())
        .unwrap_or_default();

    // e. Load messages with tool executions from DB for the conversation history.
    let messages_with_execs = params
        .repos
        .tool_executions()
        .list_messages_with_executions(
            params.conversation_id,
            params.config.conversation_history_limit,
        )
        .await
        .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;

    let history_messages = history::to_llm_messages(&messages_with_execs);

    Ok((system_prompt, history_messages))
}

/// Attempts to resolve a dynamic LLM engine from user-stored keys.
///
/// Returns `Some(engine)` if the user has a stored `llm_provider` secret
/// that decrypts successfully and differs from the system default.
async fn try_resolve_dynamic_engine<R: AgentRepos>(
    repos: &Arc<R>,
    user_id: UserId,
    conversation_id: ConversationId,
    llm_config: &Option<LlmConfig>,
    mek: &Option<Arc<Mek>>,
) -> Option<OpenAiCompatibleEngine> {
    use sober_llm::LlmKeyResolver;

    let config = llm_config.as_ref()?;
    let mek = mek.as_ref()?;

    let resolver = LlmKeyResolver::new(repos.secrets().clone(), Arc::clone(mek), config.clone());

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

/// Auto-generates a conversation title if one doesn't exist yet.
async fn auto_generate_title<R: AgentRepos>(
    params: &TurnParams<'_, R>,
    conv_id_str: &str,
    assistant_text: &str,
) {
    if assistant_text.is_empty() {
        return;
    }

    let conversation = params
        .repos
        .conversations()
        .get_by_id(params.conversation_id)
        .await
        .ok();
    let current_title = conversation.as_ref().and_then(|c| c.title.as_deref());
    let needs_title = current_title.is_none() || current_title == Some("");

    if !needs_title {
        return;
    }

    let title_prompt = format!(
        "Generate a short title (max 6 words) for a conversation that starts with:\n\
         User: {}\n\
         Assistant: {}\n\n\
         Reply with ONLY the title, no quotes or punctuation at the end.",
        params.content,
        &assistant_text[..assistant_text.len().min(200)]
    );

    let title_req = CompletionRequest {
        model: params.config.model.clone(),
        messages: vec![LlmMessage::user(title_prompt)],
        tools: vec![],
        max_tokens: Some(200),
        temperature: Some(0.3),
        stop: vec![],
        stream: false,
    };

    // Use non-streaming complete() for title generation — it's a short call.
    if let Ok(title_resp) = params.llm.complete(title_req).await
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
                    .filter_map(|line| line.strip_prefix(" - ").or(line.strip_prefix("- ")))
                    .find(|candidate| {
                        let words = candidate.split_whitespace().count();
                        (2..=6).contains(&words)
                    })
                    .map(|t| t.trim().trim_matches('"').to_owned())
            })
        })
        && params
            .repos
            .conversations()
            .update_title(params.conversation_id, &title)
            .await
            .is_ok()
    {
        let _ = params
            .event_tx
            .send(Ok(AgentEvent::TitleGenerated(title.clone())))
            .await;
        let _ = params.broadcast_tx.send(proto::ConversationUpdate {
            conversation_id: conv_id_str.to_owned(),
            event: Some(proto::conversation_update::Event::TitleChanged(
                proto::TitleChanged { title },
            )),
        });
    }
}
