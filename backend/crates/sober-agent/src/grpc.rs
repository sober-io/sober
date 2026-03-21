//! gRPC service implementation for the agent.
//!
//! Bridges between the tonic-generated proto types and the [`Agent`] struct.

use std::sync::Arc;

use sober_core::types::AgentRepos;
use sober_core::types::JobPayload;
use sober_core::types::ids::{ConversationId, UserId, WorkspaceId};
use sober_core::types::repo::ConversationRepo;
use sober_plugin::PluginManager;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{error, warn};

use crate::agent::Agent;
use crate::broadcast::ConversationUpdateSender;
use crate::confirm::ConfirmationSender;
use crate::grpc_plugins;
use crate::grpc_tasks;
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
    plugin_manager: Arc<PluginManager<R::Plg>>,
}

impl<R: AgentRepos> AgentGrpcService<R> {
    /// Creates a new gRPC service backed by the given agent.
    pub fn new(
        agent: Arc<Agent<R>>,
        confirmation_sender: ConfirmationSender,
        permission_mode: SharedPermissionMode,
        broadcast_tx: ConversationUpdateSender,
        plugin_manager: Arc<PluginManager<R::Plg>>,
    ) -> Self {
        Self {
            agent,
            confirmation_sender,
            permission_mode,
            broadcast_tx,
            plugin_manager,
        }
    }

    /// Returns a reference to the wrapped agent.
    pub(crate) fn agent(&self) -> &Arc<Agent<R>> {
        &self.agent
    }
}

/// Resolved workspace context from a conversation ID.
pub(crate) struct WorkspaceContext {
    pub(crate) workspace_id: Option<WorkspaceId>,
    pub(crate) workspace_dir: std::path::PathBuf,
}

impl<R: AgentRepos> AgentGrpcService<R> {
    /// Resolves workspace ID and directory from an optional conversation ID string.
    pub(crate) async fn resolve_workspace_context(
        &self,
        conversation_id: Option<&str>,
    ) -> WorkspaceContext {
        let Some(conv_id_str) = conversation_id else {
            return WorkspaceContext {
                workspace_id: None,
                workspace_dir: std::path::PathBuf::new(),
            };
        };

        let Ok(uuid) = conv_id_str.parse::<uuid::Uuid>() else {
            return WorkspaceContext {
                workspace_id: None,
                workspace_dir: std::path::PathBuf::new(),
            };
        };

        let conv_id = ConversationId::from_uuid(uuid);
        let workspace_id = self
            .agent
            .repos()
            .conversations()
            .get_by_id(conv_id)
            .await
            .ok()
            .and_then(|c| c.workspace_id);
        let workspace_dir = self
            .agent
            .resolve_workspace_dir(conv_id)
            .await
            .unwrap_or_default();

        WorkspaceContext {
            workspace_id,
            workspace_dir,
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
        // Extract trace context BEFORE creating the span so the OTel layer
        // assigns the correct trace ID (inheriting the caller's trace).
        // The guard must be dropped before any .await (it's !Send).
        let span = {
            let parent_cx = sober_core::extract_trace_context(request.metadata());
            let _guard = parent_cx.attach();
            tracing::info_span!(
                "agent.handle_message",
                otel.kind = "server",
                rpc.service = "AgentService",
                rpc.method = "HandleMessage",
                rpc.system = "grpc",
                user.id = tracing::field::Empty,
                conversation.id = tracing::field::Empty,
                message.length = tracing::field::Empty,
                trigger = "human",
                otel.status_code = tracing::field::Empty,
            )
        };
        let _enter = span.enter();

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

        span.record("user.id", user_id.to_string().as_str());
        span.record("conversation.id", conversation_id.to_string().as_str());
        span.record("message.length", req.content.len());

        let agent = Arc::clone(&self.agent);
        let content = req.content;

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
                span.record("otel.status_code", "OK");
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
                span.record("otel.status_code", "ERROR");
                error!(error.message = %e, "agent handle_message failed");
                Err(Status::internal(e.to_string()))
            }
        }
    }

    async fn execute_task(
        &self,
        request: Request<proto::ExecuteTaskRequest>,
    ) -> Result<Response<Self::ExecuteTaskStream>, Status> {
        let span = {
            let parent_cx = sober_core::extract_trace_context(request.metadata());
            let _guard = parent_cx.attach();
            tracing::info_span!(
                "agent.execute_task",
                otel.kind = "server",
                rpc.service = "AgentService",
                rpc.method = "ExecuteTask",
                rpc.system = "grpc",
                task.id = tracing::field::Empty,
                task.type = tracing::field::Empty,
                caller = tracing::field::Empty,
                otel.status_code = tracing::field::Empty,
            )
        };
        let _enter = span.enter();

        let req = request.into_inner();

        span.record("task.id", req.task_id.as_str());
        span.record("task.type", req.task_type.as_str());
        span.record("caller", req.caller_identity.as_str());

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
                    grpc_tasks::execute_typed_payload(
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

                    grpc_tasks::execute_prompt_conversational(
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
        request: Request<proto::ListSkillsRequest>,
    ) -> Result<Response<proto::ListSkillsResponse>, Status> {
        grpc_plugins::handle_list_skills(self, request).await
    }

    async fn reload_skills(
        &self,
        request: Request<proto::ReloadSkillsRequest>,
    ) -> Result<Response<proto::ReloadSkillsResponse>, Status> {
        grpc_plugins::handle_reload_skills(self, &self.plugin_manager, request).await
    }

    async fn list_plugins(
        &self,
        request: Request<proto::ListPluginsRequest>,
    ) -> Result<Response<proto::ListPluginsResponse>, Status> {
        grpc_plugins::handle_list_plugins(self, request).await
    }

    async fn install_plugin(
        &self,
        request: Request<proto::InstallPluginRequest>,
    ) -> Result<Response<proto::InstallPluginResponse>, Status> {
        grpc_plugins::handle_install_plugin(self, request).await
    }

    async fn uninstall_plugin(
        &self,
        request: Request<proto::UninstallPluginRequest>,
    ) -> Result<Response<proto::UninstallPluginResponse>, Status> {
        grpc_plugins::handle_uninstall_plugin(self, request).await
    }

    async fn enable_plugin(
        &self,
        request: Request<proto::EnablePluginRequest>,
    ) -> Result<Response<proto::EnablePluginResponse>, Status> {
        grpc_plugins::handle_enable_plugin(self, request).await
    }

    async fn disable_plugin(
        &self,
        request: Request<proto::DisablePluginRequest>,
    ) -> Result<Response<proto::DisablePluginResponse>, Status> {
        grpc_plugins::handle_disable_plugin(self, request).await
    }

    async fn import_plugins(
        &self,
        request: Request<proto::ImportPluginsRequest>,
    ) -> Result<Response<proto::ImportPluginsResponse>, Status> {
        grpc_plugins::handle_import_plugins(self, request).await
    }

    async fn reload_plugins(
        &self,
        request: Request<proto::ReloadPluginsRequest>,
    ) -> Result<Response<proto::ReloadPluginsResponse>, Status> {
        grpc_plugins::handle_reload_plugins(self, request).await
    }

    async fn change_plugin_scope(
        &self,
        request: Request<proto::ChangePluginScopeRequest>,
    ) -> Result<Response<proto::ChangePluginScopeResponse>, Status> {
        grpc_plugins::handle_change_plugin_scope(self, &self.plugin_manager, request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::AgentEvent;
    use sober_core::MessageId;

    #[test]
    fn to_proto_event_text_delta() {
        let event = AgentEvent::TextDelta("hello".to_owned());
        let proto = grpc_tasks::to_proto_event(event);
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
        let proto = grpc_tasks::to_proto_event(event);
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
        let proto = grpc_tasks::to_proto_event(event);
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
        let proto = grpc_tasks::to_proto_event(event);
        match proto.event {
            Some(proto::agent_event::Event::Error(e)) => {
                assert_eq!(e.message, "something broke");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
