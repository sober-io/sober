//! gRPC service implementation for the agent.
//!
//! Bridges between the tonic-generated proto types and the [`Agent`] struct.

use std::sync::Arc;

use sober_core::types::JobPayload;
use sober_core::types::ids::{ConversationId, UserId, WorkspaceId};
use sober_core::types::repo::{ConversationRepo, McpServerRepo, MessageRepo};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{error, warn};

use crate::agent::Agent;
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
pub struct AgentGrpcService<Msg, Conv, Mcp>
where
    Msg: MessageRepo,
    Conv: ConversationRepo,
    Mcp: McpServerRepo,
{
    agent: Arc<Agent<Msg, Conv, Mcp>>,
    confirmation_sender: ConfirmationSender,
    permission_mode: SharedPermissionMode,
}

impl<Msg, Conv, Mcp> AgentGrpcService<Msg, Conv, Mcp>
where
    Msg: MessageRepo,
    Conv: ConversationRepo,
    Mcp: McpServerRepo,
{
    /// Creates a new gRPC service backed by the given agent.
    pub fn new(
        agent: Arc<Agent<Msg, Conv, Mcp>>,
        confirmation_sender: ConfirmationSender,
        permission_mode: SharedPermissionMode,
    ) -> Self {
        Self {
            agent,
            confirmation_sender,
            permission_mode,
        }
    }
}

/// Streaming response type for `handle_message` and `execute_task`.
type HandleMessageStream = ReceiverStream<Result<proto::AgentEvent, Status>>;

#[tonic::async_trait]
impl<Msg, Conv, Mcp> proto::agent_service_server::AgentService for AgentGrpcService<Msg, Conv, Mcp>
where
    Msg: MessageRepo + 'static,
    Conv: ConversationRepo + 'static,
    Mcp: McpServerRepo + 'static,
{
    type HandleMessageStream = HandleMessageStream;
    type ExecuteTaskStream = HandleMessageStream;

    async fn handle_message(
        &self,
        request: Request<proto::HandleMessageRequest>,
    ) -> Result<Response<Self::HandleMessageStream>, Status> {
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

        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let agent = Arc::clone(&self.agent);
        let content = req.content;

        tokio::spawn(async move {
            match agent
                .handle_message(user_id, conversation_id, &content)
                .await
            {
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
                    error!(error = %e, "agent handle_message failed");
                    let proto_event = to_proto_event(AgentEvent::Error(e.to_string()));
                    let _ = tx.send(Ok(proto_event)).await;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
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
            match JobPayload::from_bytes(&payload) {
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

                    execute_prompt_fallback(
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
}

/// Executes a typed [`JobPayload`], dispatching to the appropriate handler.
async fn execute_typed_payload<Msg, Conv, Mcp>(
    agent: &Agent<Msg, Conv, Mcp>,
    payload: JobPayload,
    user_id: Option<UserId>,
    conversation_id: Option<ConversationId>,
    workspace_id: Option<WorkspaceId>,
    task_id: &str,
    tx: &tokio::sync::mpsc::Sender<Result<proto::AgentEvent, Status>>,
) where
    Msg: MessageRepo + 'static,
    Conv: ConversationRepo + 'static,
    Mcp: sober_core::types::repo::McpServerRepo + 'static,
{
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
                execute_prompt_fallback(agent, &text, Some(uid), Some(cid), task_id, tx).await;
            } else {
                // No conversation context available — send Done with no artifact.
                warn!(task_id = %task_id, "prompt job has no resolvable conversation, skipping");
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
        }
        JobPayload::Artifact {
            blob_ref,
            artifact_type,
            ..
        } => {
            // Artifact execution via sandbox — not yet implemented.
            warn!(
                task_id = %task_id,
                blob_ref = %blob_ref,
                artifact_type = ?artifact_type,
                "artifact execution not yet implemented"
            );
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
        JobPayload::Internal { operation } => {
            // Internal operations — not yet wired to crate methods.
            warn!(
                task_id = %task_id,
                operation = ?operation,
                "internal operation not yet implemented"
            );
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
    }
}

/// Legacy execution path: treat payload as a plain prompt and delegate to `handle_message`.
async fn execute_prompt_fallback<Msg, Conv, Mcp>(
    agent: &Agent<Msg, Conv, Mcp>,
    prompt: &str,
    user_id: Option<UserId>,
    conversation_id: Option<ConversationId>,
    task_id: &str,
    tx: &tokio::sync::mpsc::Sender<Result<proto::AgentEvent, Status>>,
) where
    Msg: MessageRepo + 'static,
    Conv: ConversationRepo + 'static,
    Mcp: sober_core::types::repo::McpServerRepo + 'static,
{
    let result = if let (Some(uid), Some(cid)) = (user_id, conversation_id) {
        agent.handle_message(uid, cid, prompt).await
    } else {
        // No conversation context — emit Done immediately.
        let done = to_proto_event(AgentEvent::Done {
            message_id: sober_core::MessageId::new(),
            usage: crate::stream::Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
            },
            artifact_ref: None,
        });
        let _ = tx.send(Ok(done)).await;
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

/// Converts an [`AgentEvent`] to its proto representation.
fn to_proto_event(event: AgentEvent) -> proto::AgentEvent {
    use proto::agent_event::Event;

    let inner = match event {
        AgentEvent::TextDelta(content) => Event::TextDelta(proto::TextDelta { content }),
        AgentEvent::ToolCallStart { name, input } => Event::ToolCallStart(proto::ToolCallStart {
            name,
            input_json: input.to_string(),
        }),
        AgentEvent::ToolCallResult { name, output } => {
            Event::ToolCallResult(proto::ToolCallResult { name, output })
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
