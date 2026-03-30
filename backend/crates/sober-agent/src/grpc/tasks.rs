//! Task execution helpers, extracted from [`grpc`].
//!
//! These free functions implement the logic for dispatching scheduled task
//! payloads and converting agent events to their proto representations. The
//! `execute_task` RPC handler in `grpc.rs` delegates to them, keeping the
//! main file focused on the trait impl.

use sober_core::types::AgentRepos;
use sober_core::types::JobPayload;
use sober_core::types::access::{CallerContext, TriggerKind};
use sober_core::types::enums::EvolutionStatus;
use sober_core::types::ids::{ConversationId, UserId, WorkspaceId};
use sober_core::types::repo::{ConversationRepo, EvolutionRepo, MessageRepo};
use sober_llm::Message as LlmMessage;
use sober_llm::types::{CompletionRequest, FunctionDefinition, ToolDefinition};
use tonic::Status;
use tracing::{error, info, warn};

use std::sync::Arc;

use crate::agent::Agent;
use crate::evolution::{EvolutionContext, execute_evolution};
use crate::grpc::proto;
use crate::stream::AgentEvent;
use crate::system_jobs::SELF_EVOLUTION_CHECK_PROMPT;

/// Executes a typed [`JobPayload`], dispatching to the appropriate handler.
pub(crate) async fn execute_typed_payload<R: AgentRepos>(
    agent: &Arc<Agent<R>>,
    payload: JobPayload,
    user_id: Option<UserId>,
    conversation_id: Option<ConversationId>,
    workspace_id: Option<WorkspaceId>,
    task_id: &str,
    tx: &tokio::sync::mpsc::Sender<Result<proto::AgentEvent, Status>>,
) {
    match payload {
        JobPayload::Prompt { ref text, .. } if text == SELF_EVOLUTION_CHECK_PROMPT => {
            // Custom handler: 4-phase self-evolution cycle.
            execute_self_evolution_check(agent, task_id, tx).await;
        }
        JobPayload::Prompt { text, .. } => {
            // Resolve delivery conversation for the result.
            let resolved_cid = if let Some(uid) = user_id {
                agent
                    .resolve_delivery_conversation(conversation_id, uid, workspace_id)
                    .await
            } else {
                conversation_id
            };

            // If we have a user + conversation, delegate to the conversational handler.
            if let (Some(uid), Some(cid)) = (user_id, resolved_cid) {
                execute_prompt_conversational(agent, &text, Some(uid), Some(cid), task_id, tx)
                    .await;
            } else {
                // No conversation context — use autonomous prompt assembly.
                // This validates the SOUL.md chain and prompt construction for
                // system-level scheduled jobs.
                let caller = CallerContext {
                    user_id,
                    trigger: TriggerKind::Scheduler,
                    permissions: vec![],
                    scope_grants: vec![],
                    workspace_id,
                };
                match agent
                    .mind()
                    .assemble_autonomous_prompt(&text, &caller)
                    .await
                {
                    Ok(_messages) => {
                        // TODO: feed messages to LLM engine and stream response
                        // For now, log that autonomous execution was assembled
                        tracing::info!(
                            task_id = %task_id,
                            "autonomous prompt assembled (LLM execution not yet wired)"
                        );
                        send_done_stub(tx).await;
                    }
                    Err(e) => {
                        let proto_event = to_proto_event(AgentEvent::Error(e.to_string()));
                        let _ = tx.send(Ok(proto_event)).await;
                    }
                }
            }
        }
        JobPayload::Artifact {
            blob_ref,
            artifact_type,
            ..
        } => {
            error!(
                task_id = %task_id,
                blob_ref = %blob_ref,
                artifact_type = ?artifact_type,
                "artifact execution not yet implemented — requires BwrapSandbox integration"
            );
            let proto_event = to_proto_event(AgentEvent::Error(
                "Artifact execution is not yet implemented".into(),
            ));
            let _ = tx.send(Ok(proto_event)).await;
        }
        JobPayload::Internal { operation } => {
            error!(
                task_id = %task_id,
                operation = ?operation,
                "internal operation not yet implemented — requires crate-level execution APIs"
            );
            let proto_event = to_proto_event(AgentEvent::Error(format!(
                "Internal operation {:?} is not yet implemented",
                operation
            )));
            let _ = tx.send(Ok(proto_event)).await;
        }
    }
}

/// Runs the 4-phase self-evolution check cycle.
///
/// This is the custom handler for the `self_evolution_check` system job.
/// Instead of sending a prompt to the LLM, it:
///
/// 1. **Execute approved** — queries approved evolution events and executes them.
/// 2. **Gather data** — queries recent conversations for pattern detection (stub).
/// 3. **Active context** — loads active evolutions for detection context.
/// 4. **Detect** — builds a structured prompt for the LLM to detect new
///    evolution opportunities (stub — logs context, skips LLM call).
///
/// After detection, executes any newly auto-approved events.
async fn execute_self_evolution_check<R: AgentRepos>(
    agent: &Arc<Agent<R>>,
    task_id: &str,
    tx: &tokio::sync::mpsc::Sender<Result<proto::AgentEvent, Status>>,
) {
    info!(task_id = %task_id, "starting self-evolution check cycle");
    let cycle_start = std::time::Instant::now();

    let evo_ctx = EvolutionContext {
        scheduler_client: std::sync::Arc::clone(&agent.tool_bootstrap().scheduler_client),
        plugin_manager: std::sync::Arc::clone(&agent.tool_bootstrap().plugin_manager),
        plugin_generator: agent.tool_bootstrap().plugin_generator.clone(),
    };

    // -----------------------------------------------------------------------
    // Phase 1: Execute pending approved evolution events
    // -----------------------------------------------------------------------
    info!(task_id = %task_id, "phase 1: executing approved evolution events");

    let approved_events = match agent
        .repos()
        .evolution()
        .list(None, Some(EvolutionStatus::Approved))
        .await
    {
        Ok(events) => events,
        Err(e) => {
            warn!(task_id = %task_id, error = %e, "failed to query approved evolution events");
            vec![]
        }
    };

    let approved_count = approved_events.len();
    if approved_count > 0 {
        info!(
            task_id = %task_id,
            count = approved_count,
            "found approved evolution events to execute"
        );
    }

    for event in &approved_events {
        if let Err(e) = execute_evolution(event, agent.repos(), agent.mind(), &evo_ctx).await {
            warn!(
                task_id = %task_id,
                event_id = %event.id,
                error = %e,
                "failed to execute approved evolution event"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Phase 2: Gather recent conversation data for pattern detection
    // -----------------------------------------------------------------------
    info!(task_id = %task_id, "phase 2: gathering conversation data");

    let conversation_summary = gather_conversation_summary(agent).await;

    // -----------------------------------------------------------------------
    // Phase 3: Load active evolutions for context
    // -----------------------------------------------------------------------
    info!(task_id = %task_id, "phase 3: loading active evolution context");

    let active_events = match agent.repos().evolution().list_active().await {
        Ok(events) => events,
        Err(e) => {
            warn!(task_id = %task_id, error = %e, "failed to query active evolution events");
            vec![]
        }
    };

    let active_context = if active_events.is_empty() {
        "No active evolutions.".to_owned()
    } else {
        active_events
            .iter()
            .map(|e| {
                format!(
                    "- [{}] {} (type: {:?}, since: {})",
                    e.id, e.title, e.evolution_type, e.updated_at
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    // -----------------------------------------------------------------------
    // Phase 4: Detection — build prompt and call LLM
    // -----------------------------------------------------------------------
    info!(
        task_id = %task_id,
        active_evolutions = active_events.len(),
        "phase 4: running LLM detection"
    );

    run_detection_llm(agent, &conversation_summary, &active_context, task_id).await;

    // -----------------------------------------------------------------------
    // Post-detection: execute any newly auto-approved events
    // -----------------------------------------------------------------------
    let post_approved = match agent
        .repos()
        .evolution()
        .list(None, Some(EvolutionStatus::Approved))
        .await
    {
        Ok(events) => events,
        Err(e) => {
            warn!(task_id = %task_id, error = %e, "failed to query post-detection approved events");
            vec![]
        }
    };

    let new_approved: Vec<_> = post_approved
        .iter()
        .filter(|e| !approved_events.iter().any(|prev| prev.id == e.id))
        .collect();

    if !new_approved.is_empty() {
        info!(
            task_id = %task_id,
            count = new_approved.len(),
            "executing newly auto-approved evolution events"
        );
        for event in new_approved {
            if let Err(e) = execute_evolution(event, agent.repos(), agent.mind(), &evo_ctx).await {
                warn!(
                    task_id = %task_id,
                    event_id = %event.id,
                    error = %e,
                    "failed to execute newly auto-approved evolution event"
                );
            }
        }
    }

    metrics::histogram!("sober_evolution_cycle_duration_seconds")
        .record(cycle_start.elapsed().as_secs_f64());

    info!(task_id = %task_id, "self-evolution check cycle complete");
    send_done_stub(tx).await;
}

/// Executes a prompt payload by delegating to `handle_message` with conversation context.
pub(crate) async fn execute_prompt_conversational<R: AgentRepos>(
    agent: &Arc<Agent<R>>,
    prompt: &str,
    user_id: Option<UserId>,
    conversation_id: Option<ConversationId>,
    task_id: &str,
    tx: &tokio::sync::mpsc::Sender<Result<proto::AgentEvent, Status>>,
) {
    let result = if let (Some(uid), Some(cid)) = (user_id, conversation_id) {
        agent
            .handle_message(
                uid,
                cid,
                prompt,
                sober_core::types::access::TriggerKind::Scheduler,
            )
            .await
    } else {
        // No conversation context — emit Done immediately.
        send_done_stub(tx).await;
        return;
    };

    match result {
        Ok(mut stream) => {
            use futures::StreamExt;
            while let Some(event_result) = stream.next().await {
                let proto_event = match event_result {
                    Ok(event) => to_proto_event(event),
                    Err(e) => to_proto_event(AgentEvent::Error(e.to_string())),
                };
                if tx.send(Ok(proto_event)).await.is_err() {
                    break;
                }
            }
        }
        Err(e) => {
            error!(error = %e, task_id = %task_id, "task execution failed");
            let proto_event = to_proto_event(AgentEvent::Error(e.to_string()));
            let _ = tx.send(Ok(proto_event)).await;
        }
    }
}

/// Sends a no-op Done event (zero tokens, no artifact).
pub(crate) async fn send_done_stub(
    tx: &tokio::sync::mpsc::Sender<Result<proto::AgentEvent, Status>>,
) {
    let done = to_proto_event(AgentEvent::Done {
        message_id: sober_core::MessageId::new(),
        usage: crate::stream::Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
        },
        artifact_ref: None,
    });
    let _ = tx.send(Ok(done)).await;
}

// ---------------------------------------------------------------------------
// Self-evolution detection helpers
// ---------------------------------------------------------------------------

/// Maximum recent conversations to query for pattern detection.
const DETECTION_CONV_LIMIT: i64 = 20;
/// Maximum messages to sample per conversation.
const DETECTION_MSG_LIMIT: i64 = 10;

/// Gathers a compact summary of recent conversation activity for the LLM.
async fn gather_conversation_summary<R: AgentRepos>(agent: &Arc<Agent<R>>) -> String {
    let recent_convs = match agent
        .repos()
        .conversations()
        .list_recent(DETECTION_CONV_LIMIT)
        .await
    {
        Ok(convs) => convs,
        Err(e) => {
            warn!(error = %e, "failed to query recent conversations for detection");
            return "No conversation data available.".to_owned();
        }
    };

    if recent_convs.is_empty() {
        return "No recent conversations.".to_owned();
    }

    let mut lines = Vec::with_capacity(recent_convs.len());

    for conv in &recent_convs {
        let messages = match agent
            .repos()
            .messages()
            .list_by_conversation(conv.id, DETECTION_MSG_LIMIT)
            .await
        {
            Ok(msgs) => msgs,
            Err(_) => continue,
        };

        let msg_count = messages.len();
        let user_msgs = messages
            .iter()
            .filter(|m| m.role == sober_core::types::enums::MessageRole::User)
            .count();
        let tool_calls: Vec<&str> = messages
            .iter()
            .filter_map(|m| m.metadata.as_ref())
            .filter_map(|meta| meta.get("tool_calls"))
            .filter_map(|v| v.as_array())
            .flat_map(|arr| arr.iter())
            .filter_map(|tc| tc.get("name"))
            .filter_map(|v| v.as_str())
            .collect();

        let title = conv.title.as_deref().unwrap_or("untitled");
        let updated = conv.updated_at.format("%Y-%m-%d %H:%M");

        if tool_calls.is_empty() {
            lines.push(format!(
                "- {title} (user: {}, msgs: {msg_count}, user_msgs: {user_msgs}, updated: {updated})",
                conv.user_id
            ));
        } else {
            // Deduplicate tool names and count.
            let mut tool_counts: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
            for name in &tool_calls {
                *tool_counts.entry(name).or_default() += 1;
            }
            let tools_str: String = tool_counts
                .iter()
                .map(|(name, count)| format!("{name}×{count}"))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!(
                "- {title} (user: {}, msgs: {msg_count}, tools: [{tools_str}], updated: {updated})",
                conv.user_id
            ));
        }
    }

    lines.join("\n")
}

/// The system prompt for the self-evolution detection LLM call.
const DETECTION_SYSTEM_PROMPT: &str = "\
You are the self-evolution engine for Sõber, an AI agent system. Your job is to \
analyse recent conversation patterns and propose improvements.

You have access to four proposal tools:
- propose_tool: Propose a new WASM plugin tool when users repeatedly need a capability that doesn't exist.
- propose_skill: Propose a new prompt-based skill when users frequently request a specific type of assistance.
- propose_instruction: Propose an instruction overlay change when the agent's behavior should be adjusted.
- propose_automation: Propose a scheduled job when users have recurring needs at predictable intervals.

Rules:
1. Only propose evolutions backed by clear patterns in the data — do not speculate.
2. Do not duplicate existing active evolutions (listed below).
3. Limit to at most 5 proposals per cycle.
4. Each proposal must include a confidence score (0.0–1.0) and source_count.
5. If no patterns warrant a proposal, respond with a brief summary and make no tool calls.
";

/// Runs the LLM detection call with conversation and evolution context.
///
/// Builds a prompt from gathered data, calls the LLM with propose_* tool
/// definitions, then dispatches any returned tool calls to the propose_*
/// tool implementations.
async fn run_detection_llm<R: AgentRepos>(
    agent: &Arc<Agent<R>>,
    conversation_summary: &str,
    active_context: &str,
    task_id: &str,
) {
    // Build the user message with injected context.
    let user_message = format!(
        "Analyse the following recent conversation activity and active evolutions. \
         Propose improvements if patterns warrant them.\n\n\
         ## Recent conversation activity\n\n{conversation_summary}\n\n\
         ## Active evolutions\n\n{active_context}"
    );

    // Collect propose_* tool definitions from the tool bootstrap.
    let propose_tools = build_propose_tool_definitions(agent);

    let model = agent
        .llm_config()
        .as_ref()
        .map(|c| c.model.clone())
        .unwrap_or_else(|| "default".to_owned());

    let req = CompletionRequest {
        model,
        messages: vec![
            LlmMessage::system(DETECTION_SYSTEM_PROMPT),
            LlmMessage::user(&user_message),
        ],
        tools: propose_tools,
        max_tokens: Some(4096),
        temperature: Some(0.3),
        stop: vec![],
        stream: false,
    };

    let response = match agent.llm().complete(req).await {
        Ok(resp) => resp,
        Err(e) => {
            warn!(task_id = %task_id, error = %e, "detection LLM call failed");
            return;
        }
    };

    // Process tool calls from the response.
    let tool_calls = response
        .choices
        .first()
        .and_then(|c| c.message.tool_calls.as_ref())
        .cloned()
        .unwrap_or_default();

    if tool_calls.is_empty() {
        let text = response
            .choices
            .first()
            .and_then(|c| c.message.content.as_deref())
            .unwrap_or("no response");
        info!(
            task_id = %task_id,
            response = %text,
            "detection LLM proposed no evolutions"
        );
        return;
    }

    info!(
        task_id = %task_id,
        count = tool_calls.len(),
        "detection LLM proposed evolutions"
    );

    // Dispatch each tool call to the matching propose_* tool.
    let tools = agent.tool_bootstrap().build_static_tools();
    for tc in &tool_calls {
        let tool_name = &tc.function.name;
        let input = match serde_json::from_str::<serde_json::Value>(&tc.function.arguments) {
            Ok(v) => v,
            Err(e) => {
                warn!(task_id = %task_id, tool = %tool_name, error = %e, "invalid tool call arguments");
                continue;
            }
        };

        let tool = tools.iter().find(|t| t.metadata().name == *tool_name);
        if let Some(tool) = tool {
            match tool.execute(input).await {
                Ok(output) => {
                    info!(
                        task_id = %task_id,
                        tool = %tool_name,
                        output = %output.content,
                        "detection proposal created"
                    );
                }
                Err(e) => {
                    warn!(
                        task_id = %task_id,
                        tool = %tool_name,
                        error = %e,
                        "detection proposal tool call failed"
                    );
                }
            }
        } else {
            warn!(
                task_id = %task_id,
                tool = %tool_name,
                "detection LLM called unknown tool"
            );
        }
    }
}

/// Builds LLM tool definitions for the propose_* tools.
fn build_propose_tool_definitions<R: AgentRepos>(agent: &Arc<Agent<R>>) -> Vec<ToolDefinition> {
    let all_tools = agent.tool_bootstrap().build_static_tools();
    all_tools
        .iter()
        .filter(|t| t.metadata().name.starts_with("propose_"))
        .map(|t| {
            let meta = t.metadata();
            ToolDefinition {
                r#type: "function".to_owned(),
                function: FunctionDefinition {
                    name: meta.name,
                    description: meta.description,
                    parameters: meta.input_schema,
                },
            }
        })
        .collect()
}

/// Converts an [`AgentEvent`] to its proto representation.
pub(crate) fn to_proto_event(event: AgentEvent) -> proto::AgentEvent {
    use proto::agent_event::Event;

    let inner = match event {
        AgentEvent::TextDelta(content) => Event::TextDelta(proto::TextDelta { content }),
        AgentEvent::ThinkingDelta(content) => {
            Event::ThinkingDelta(proto::ThinkingDelta { content })
        }
        AgentEvent::ToolExecutionUpdate {
            id,
            message_id,
            tool_call_id,
            tool_name,
            status,
            output,
            error,
            input,
        } => Event::ToolExecutionUpdate(proto::ToolExecutionUpdate {
            id,
            message_id,
            tool_call_id,
            tool_name,
            status,
            output,
            error,
            input,
        }),
        AgentEvent::Done {
            message_id,
            usage,
            artifact_ref,
        } => Event::Done(proto::Done {
            message_id: message_id.to_string(),
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            artifact_ref: artifact_ref.unwrap_or_default(),
        }),
        AgentEvent::TitleGenerated(title) => Event::TitleGenerated(proto::TitleGenerated { title }),
        AgentEvent::ConfirmRequest {
            confirm_id,
            command,
            risk_level,
            affects,
            reason,
        } => Event::ConfirmRequest(proto::ConfirmRequest {
            confirm_id,
            command,
            risk_level,
            affects,
            reason,
        }),
        AgentEvent::Error(message) => Event::Error(proto::Error { message }),
    };

    proto::AgentEvent { event: Some(inner) }
}
