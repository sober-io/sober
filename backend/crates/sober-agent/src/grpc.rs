//! gRPC service implementation for the agent.
//!
//! Bridges between the tonic-generated proto types and the [`Agent`] struct.

use std::sync::Arc;

use sober_core::types::AgentRepos;
use sober_core::types::JobPayload;
use sober_core::types::access::{CallerContext, TriggerKind};
use sober_core::types::ids::{ConversationId, UserId, WorkspaceId};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{error, warn};

use crate::agent::Agent;
use crate::broadcast::ConversationUpdateSender;
use crate::confirm::ConfirmationSender;
use crate::stream::AgentEvent;
use crate::tools::SharedPermissionMode;

/// Generated protobuf types for the agent gRPC service.
pub mod proto {
    tonic::include_proto!("sober.agent.v1");
}

/// Generated protobuf types for the scheduler gRPC service (client-side).
pub mod scheduler_proto {
    tonic::include_proto!("sober.scheduler.v1");
}

/// gRPC service wrapping an [`Agent`].
pub struct AgentGrpcService<R: AgentRepos> {
    agent: Arc<Agent<R>>,
    confirmation_sender: ConfirmationSender,
    permission_mode: SharedPermissionMode,
    broadcast_tx: ConversationUpdateSender,
}

impl<R: AgentRepos> AgentGrpcService<R> {
    /// Creates a new gRPC service backed by the given agent.
    pub fn new(
        agent: Arc<Agent<R>>,
        confirmation_sender: ConfirmationSender,
        permission_mode: SharedPermissionMode,
        broadcast_tx: ConversationUpdateSender,
    ) -> Self {
        Self {
            agent,
            confirmation_sender,
            permission_mode,
            broadcast_tx,
        }
    }
}

/// Streaming response type for `execute_task`.
type ExecuteTaskStream = ReceiverStream<Result<proto::AgentEvent, Status>>;

/// Streaming response type for `subscribe_conversation_updates`.
type SubscribeConversationUpdatesStream = ReceiverStream<Result<proto::ConversationUpdate, Status>>;

#[tonic::async_trait]
impl<R: AgentRepos> proto::agent_service_server::AgentService for AgentGrpcService<R> {
    type ExecuteTaskStream = ExecuteTaskStream;
    type SubscribeConversationUpdatesStream = SubscribeConversationUpdatesStream;

    async fn handle_message(
        &self,
        request: Request<proto::HandleMessageRequest>,
    ) -> Result<Response<proto::HandleMessageResponse>, Status> {
        let req = request.into_inner();

        let user_id = req
            .user_id
            .parse::<uuid::Uuid>()
            .map(UserId::from_uuid)
            .map_err(|_| Status::invalid_argument("invalid user_id"))?;

        let conversation_id = req
            .conversation_id
            .parse::<uuid::Uuid>()
            .map(ConversationId::from_uuid)
            .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;

        let agent = Arc::clone(&self.agent);
        let content = req.content;

        // handle_message stores the user message, spawns the agentic loop
        // (which publishes to the broadcast channel), and returns the stream.
        // We consume the stream to drive the loop but don't forward it — events
        // go through the broadcast channel to SubscribeConversationUpdates.
        match agent
            .handle_message(
                user_id,
                conversation_id,
                &content,
                sober_core::types::access::TriggerKind::Human,
            )
            .await
        {
            Ok(stream) => {
                // The stream must be consumed to drive the spawned task, but
                // we don't need its output — the broadcast channel delivers
                // events. Spawn a drainer task.
                tokio::spawn(async move {
                    use futures::StreamExt;
                    let mut stream = stream;
                    while stream.next().await.is_some() {}
                });

                // Return a placeholder message_id. The actual user message ID
                // is not directly available from handle_message's current API,
                // so we return a new UUID. The frontend uses Done.message_id
                // for the assistant message.
                Ok(Response::new(proto::HandleMessageResponse {
                    message_id: sober_core::MessageId::new().to_string(),
                }))
            }
            Err(e) => {
                error!(error = %e, "agent handle_message failed");
                Err(Status::internal(e.to_string()))
            }
        }
    }

    async fn execute_task(
        &self,
        request: Request<proto::ExecuteTaskRequest>,
    ) -> Result<Response<Self::ExecuteTaskStream>, Status> {
        let req = request.into_inner();

        let user_id = req
            .user_id
            .map(|s| {
                s.parse::<uuid::Uuid>()
                    .map(UserId::from_uuid)
                    .map_err(|_| Status::invalid_argument("invalid user_id"))
            })
            .transpose()?;

        let conversation_id = req
            .conversation_id
            .map(|s| {
                s.parse::<uuid::Uuid>()
                    .map(ConversationId::from_uuid)
                    .map_err(|_| Status::invalid_argument("invalid conversation_id"))
            })
            .transpose()?;

        let workspace_id = req
            .workspace_id
            .map(|s| {
                s.parse::<uuid::Uuid>()
                    .map(WorkspaceId::from_uuid)
                    .map_err(|_| Status::invalid_argument("invalid workspace_id"))
            })
            .transpose()?;

        tracing::info!(
            task_id = %req.task_id,
            task_type = %req.task_type,
            caller = %req.caller_identity,
            user_id = ?user_id,
            conversation_id = ?conversation_id,
            workspace_id = ?workspace_id,
            payload_len = req.payload.len(),
            "executing task"
        );

        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let agent = Arc::clone(&self.agent);
        let task_id = req.task_id;
        let task_type = req.task_type;
        let payload = req.payload;

        tokio::spawn(async move {
            // Try to deserialize as a typed JobPayload; fall back to raw prompt.
            match serde_json::from_slice::<JobPayload>(&payload) {
                Ok(job_payload) => {
                    execute_typed_payload(
                        &agent,
                        job_payload,
                        user_id,
                        conversation_id,
                        workspace_id,
                        &task_id,
                        &tx,
                    )
                    .await;
                }
                Err(_) => {
                    // Legacy path: treat payload as a UTF-8 prompt string.
                    let prompt = match String::from_utf8(payload) {
                        Ok(s) if !s.is_empty() => s,
                        _ => format!("Execute scheduled task: {task_type} (id: {task_id})"),
                    };

                    execute_prompt_conversational(
                        &agent,
                        &prompt,
                        user_id,
                        conversation_id,
                        &task_id,
                        &tx,
                    )
                    .await;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn subscribe_conversation_updates(
        &self,
        _request: Request<proto::SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeConversationUpdatesStream>, Status> {
        let mut rx = self.broadcast_tx.subscribe();
        let (tx, out_rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(update) => {
                        if tx.send(Ok(update)).await.is_err() {
                            // Client disconnected.
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "subscription lagged, some events were dropped");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(out_rx)))
    }

    async fn wake_agent(
        &self,
        _request: Request<proto::WakeRequest>,
    ) -> Result<Response<proto::WakeResponse>, Status> {
        Ok(Response::new(proto::WakeResponse { accepted: true }))
    }

    async fn submit_confirmation(
        &self,
        request: Request<proto::ConfirmResponse>,
    ) -> Result<Response<proto::ConfirmAck>, Status> {
        let req = request.into_inner();
        self.confirmation_sender
            .respond(req.confirm_id, req.approved)
            .await
            .map_err(|e| Status::internal(format!("failed to forward confirmation: {e}")))?;
        Ok(Response::new(proto::ConfirmAck {}))
    }

    async fn set_permission_mode(
        &self,
        request: Request<proto::SetPermissionModeRequest>,
    ) -> Result<Response<proto::SetPermissionModeResponse>, Status> {
        let mode_str = request.into_inner().mode;
        let mode = match mode_str.as_str() {
            "interactive" => sober_core::PermissionMode::Interactive,
            "policy_based" => sober_core::PermissionMode::PolicyBased,
            "autonomous" => sober_core::PermissionMode::Autonomous,
            other => {
                return Err(Status::invalid_argument(format!(
                    "unknown permission mode: {other}"
                )));
            }
        };

        {
            let mut current = self
                .permission_mode
                .write()
                .expect("permission mode lock poisoned");
            *current = mode;
        }

        tracing::info!(mode = ?mode, "permission mode updated");
        Ok(Response::new(proto::SetPermissionModeResponse {}))
    }

    async fn health(
        &self,
        _request: Request<proto::HealthRequest>,
    ) -> Result<Response<proto::HealthResponse>, Status> {
        Ok(Response::new(proto::HealthResponse {
            healthy: true,
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }))
    }

    async fn list_skills(
        &self,
        _request: Request<proto::ListSkillsRequest>,
    ) -> Result<Response<proto::ListSkillsResponse>, Status> {
        // TODO: wire up to SkillCatalog once agent integration is complete (Task 8).
        Ok(Response::new(proto::ListSkillsResponse { skills: vec![] }))
    }
}

/// Executes a typed [`JobPayload`], dispatching to the appropriate handler.
async fn execute_typed_payload<R: AgentRepos>(
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
async fn execute_prompt_conversational<R: AgentRepos>(
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
async fn send_done_stub(tx: &tokio::sync::mpsc::Sender<Result<proto::AgentEvent, Status>>) {
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
fn to_proto_event(event: AgentEvent) -> proto::AgentEvent {
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

#[cfg(test)]
mod tests {
    use super::*;
    use sober_core::MessageId;

    #[test]
    fn to_proto_event_text_delta() {
        let event = AgentEvent::TextDelta("hello".to_owned());
        let proto = to_proto_event(event);
        match proto.event {
            Some(proto::agent_event::Event::TextDelta(td)) => {
                assert_eq!(td.content, "hello");
            }
            other => panic!("expected TextDelta, got {other:?}"),
        }
    }

    #[test]
    fn to_proto_event_tool_call_start() {
        let event = AgentEvent::ToolCallStart {
            name: "web_search".to_owned(),
            input: serde_json::json!({"query": "rust"}),
        };
        let proto = to_proto_event(event);
        match proto.event {
            Some(proto::agent_event::Event::ToolCallStart(tcs)) => {
                assert_eq!(tcs.name, "web_search");
                assert!(tcs.input_json.contains("rust"));
            }
            other => panic!("expected ToolCallStart, got {other:?}"),
        }
    }

    #[test]
    fn to_proto_event_done() {
        let event = AgentEvent::Done {
            message_id: MessageId::new(),
            usage: crate::stream::Usage {
                prompt_tokens: 100,
                completion_tokens: 50,
            },
            artifact_ref: None,
        };
        let proto = to_proto_event(event);
        match proto.event {
            Some(proto::agent_event::Event::Done(d)) => {
                assert_eq!(d.prompt_tokens, 100);
                assert_eq!(d.completion_tokens, 50);
                assert!(!d.message_id.is_empty());
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn to_proto_event_error() {
        let event = AgentEvent::Error("something broke".to_owned());
        let proto = to_proto_event(event);
        match proto.event {
            Some(proto::agent_event::Event::Error(e)) => {
                assert_eq!(e.message, "something broke");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
