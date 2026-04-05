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
use sober_core::types::repo::EvolutionRepo;
use tonic::Status;
use tracing::{error, info, warn};

use std::sync::Arc;

use crate::agent::Agent;
use crate::evolution::detection::{gather_conversation_summary, run_detection_llm};
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
/// 2. **Gather data** — queries recent conversations for pattern detection.
/// 3. **Active context** — loads active evolutions for detection context.
/// 4. **Detect** — calls the LLM with a structured prompt and propose_* tools
///    to detect new evolution opportunities from conversation patterns.
///
/// After detection, executes any newly auto-approved events.
async fn execute_self_evolution_check<R: AgentRepos>(
    agent: &Arc<Agent<R>>,
    task_id: &str,
    tx: &tokio::sync::mpsc::Sender<Result<proto::AgentEvent, Status>>,
) {
    info!(task_id = %task_id, "starting self-evolution check cycle");
    let cycle_start = std::time::Instant::now();

    let Some(plugin_generator) = agent.tool_bootstrap().plugin_generator.clone() else {
        warn!(task_id = %task_id, "plugin generator not configured — skipping evolution cycle");
        send_done_stub(tx).await;
        return;
    };

    let evo_ctx = EvolutionContext {
        scheduler_client: std::sync::Arc::clone(&agent.tool_bootstrap().scheduler_client),
        plugin_manager: std::sync::Arc::clone(&agent.tool_bootstrap().plugin_manager),
        plugin_generator,
        evolution_config: agent.tool_bootstrap().evolution_config.clone(),
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

    let conversation_summary = gather_conversation_summary(agent, &evo_ctx.evolution_config).await;

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

    run_detection_llm(
        agent,
        &evo_ctx.evolution_config,
        &conversation_summary,
        &active_context,
        task_id,
    )
    .await;

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
        let content = vec![sober_core::types::ContentBlock::text(prompt)];
        agent
            .handle_message(
                uid,
                cid,
                &content,
                sober_core::types::access::TriggerKind::Scheduler,
                "scheduler".to_owned(),
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
        content: None,
        usage: crate::stream::Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
        },
        artifact_ref: None,
    });
    let _ = tx.send(Ok(done)).await;
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
            content,
            usage,
            artifact_ref,
        } => Event::Done(proto::Done {
            message_id: message_id.to_string(),
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            artifact_ref: artifact_ref.unwrap_or_default(),
            content: content.unwrap_or_default(),
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
