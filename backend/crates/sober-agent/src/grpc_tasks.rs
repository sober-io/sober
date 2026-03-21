//! Task execution helpers, extracted from [`grpc`].
//!
//! These free functions implement the logic for dispatching scheduled task
//! payloads and converting agent events to their proto representations. The
//! `execute_task` RPC handler in `grpc.rs` delegates to them, keeping the
//! main file focused on the trait impl.

use sober_core::types::AgentRepos;
use sober_core::types::JobPayload;
use sober_core::types::access::{CallerContext, TriggerKind};
use sober_core::types::ids::{ConversationId, UserId, WorkspaceId};
use tonic::Status;
use tracing::error;

use crate::agent::Agent;
use crate::grpc::proto;
use crate::stream::AgentEvent;

/// Executes a typed [`JobPayload`], dispatching to the appropriate handler.
pub(crate) async fn execute_typed_payload<R: AgentRepos>(
    agent: &Agent<R>,
    payload: JobPayload,
    user_id: Option<UserId>,
    conversation_id: Option<ConversationId>,
    workspace_id: Option<WorkspaceId>,
    task_id: &str,
    tx: &tokio::sync::mpsc::Sender<Result<proto::AgentEvent, Status>>,
) {
    match payload {
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
                // system-level scheduled jobs (e.g. trait_evolution_check).
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

/// Executes a prompt payload by delegating to `handle_message` with conversation context.
pub(crate) async fn execute_prompt_conversational<R: AgentRepos>(
    agent: &Agent<R>,
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

/// Converts an [`AgentEvent`] to its proto representation.
pub(crate) fn to_proto_event(event: AgentEvent) -> proto::AgentEvent {
    use proto::agent_event::Event;

    let inner = match event {
        AgentEvent::TextDelta(content) => Event::TextDelta(proto::TextDelta { content }),
        AgentEvent::ToolCallStart { name, input } => Event::ToolCallStart(proto::ToolCallStart {
            name,
            input_json: input.to_string(),
            internal: false,
        }),
        AgentEvent::ToolCallResult { name, output } => {
            Event::ToolCallResult(proto::ToolCallResult {
                name,
                output,
                internal: false,
            })
        }
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
