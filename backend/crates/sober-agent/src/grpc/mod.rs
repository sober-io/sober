//! gRPC service implementation for the agent.
//!
//! Bridges between the tonic-generated proto types and the [`Agent`] struct.

use std::sync::Arc;

use sober_core::types::AgentRepos;
use sober_core::types::ids::{ConversationId, EvolutionEventId, WorkspaceId};
use sober_core::types::repo::{ConversationRepo, EvolutionRepo};
use sober_plugin::PluginManager;
use tonic::{Request, Response, Status};

mod agent;
mod plugins;
mod tasks;

use crate::agent::Agent;
use crate::broadcast::ConversationUpdateSender;
use crate::confirm::ConfirmationSender;
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

    /// Returns a reference to the conversation-update broadcast sender.
    pub(crate) fn broadcast_tx(&self) -> &ConversationUpdateSender {
        &self.broadcast_tx
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

#[tonic::async_trait]
impl<R: AgentRepos> proto::agent_service_server::AgentService for AgentGrpcService<R> {
    type ExecuteTaskStream = agent::ExecuteTaskStream;
    type SubscribeConversationUpdatesStream = agent::SubscribeConversationUpdatesStream;

    async fn handle_message(
        &self,
        request: Request<proto::HandleMessageRequest>,
    ) -> Result<Response<proto::HandleMessageResponse>, Status> {
        agent::handle_message(self, request).await
    }

    async fn execute_task(
        &self,
        request: Request<proto::ExecuteTaskRequest>,
    ) -> Result<Response<Self::ExecuteTaskStream>, Status> {
        agent::execute_task(self, request).await
    }

    async fn subscribe_conversation_updates(
        &self,
        request: Request<proto::SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeConversationUpdatesStream>, Status> {
        agent::subscribe_conversation_updates(self, request).await
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
        plugins::handle_list_skills(self, request).await
    }

    async fn reload_skills(
        &self,
        request: Request<proto::ReloadSkillsRequest>,
    ) -> Result<Response<proto::ReloadSkillsResponse>, Status> {
        plugins::handle_reload_skills(self, &self.plugin_manager, request).await
    }

    async fn list_plugins(
        &self,
        request: Request<proto::ListPluginsRequest>,
    ) -> Result<Response<proto::ListPluginsResponse>, Status> {
        plugins::handle_list_plugins(self, request).await
    }

    async fn install_plugin(
        &self,
        request: Request<proto::InstallPluginRequest>,
    ) -> Result<Response<proto::InstallPluginResponse>, Status> {
        plugins::handle_install_plugin(self, request).await
    }

    async fn uninstall_plugin(
        &self,
        request: Request<proto::UninstallPluginRequest>,
    ) -> Result<Response<proto::UninstallPluginResponse>, Status> {
        plugins::handle_uninstall_plugin(self, request).await
    }

    async fn enable_plugin(
        &self,
        request: Request<proto::EnablePluginRequest>,
    ) -> Result<Response<proto::EnablePluginResponse>, Status> {
        plugins::handle_enable_plugin(self, request).await
    }

    async fn disable_plugin(
        &self,
        request: Request<proto::DisablePluginRequest>,
    ) -> Result<Response<proto::DisablePluginResponse>, Status> {
        plugins::handle_disable_plugin(self, request).await
    }

    async fn import_plugins(
        &self,
        request: Request<proto::ImportPluginsRequest>,
    ) -> Result<Response<proto::ImportPluginsResponse>, Status> {
        plugins::handle_import_plugins(self, request).await
    }

    async fn reload_plugins(
        &self,
        request: Request<proto::ReloadPluginsRequest>,
    ) -> Result<Response<proto::ReloadPluginsResponse>, Status> {
        plugins::handle_reload_plugins(self, request).await
    }

    async fn change_plugin_scope(
        &self,
        request: Request<proto::ChangePluginScopeRequest>,
    ) -> Result<Response<proto::ChangePluginScopeResponse>, Status> {
        plugins::handle_change_plugin_scope(self, &self.plugin_manager, request).await
    }

    async fn list_tools(
        &self,
        request: Request<proto::ListToolsRequest>,
    ) -> Result<Response<proto::ListToolsResponse>, Status> {
        plugins::handle_list_tools(self, request).await
    }

    async fn execute_evolution(
        &self,
        request: Request<proto::ExecuteEvolutionRequest>,
    ) -> Result<Response<proto::ExecuteEvolutionResponse>, Status> {
        let req = request.into_inner();
        let event_id = req
            .evolution_event_id
            .parse::<uuid::Uuid>()
            .map(EvolutionEventId::from_uuid)
            .map_err(|_| Status::invalid_argument("invalid evolution_event_id"))?;

        let event = self
            .agent()
            .repos()
            .evolution()
            .get_by_id(event_id)
            .await
            .map_err(|e| Status::not_found(format!("evolution event not found: {e}")))?;

        match crate::evolution::execute_evolution(&event, self.agent().repos(), self.agent().mind())
            .await
        {
            Ok(()) => Ok(Response::new(proto::ExecuteEvolutionResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(proto::ExecuteEvolutionResponse {
                success: false,
                error: e.to_string(),
            })),
        }
    }

    async fn revert_evolution(
        &self,
        request: Request<proto::RevertEvolutionRequest>,
    ) -> Result<Response<proto::RevertEvolutionResponse>, Status> {
        let req = request.into_inner();
        let event_id = req
            .evolution_event_id
            .parse::<uuid::Uuid>()
            .map(EvolutionEventId::from_uuid)
            .map_err(|_| Status::invalid_argument("invalid evolution_event_id"))?;

        let event = self
            .agent()
            .repos()
            .evolution()
            .get_by_id(event_id)
            .await
            .map_err(|e| Status::not_found(format!("evolution event not found: {e}")))?;

        match crate::evolution::revert_evolution(&event, self.agent().repos(), self.agent().mind())
            .await
        {
            Ok(()) => Ok(Response::new(proto::RevertEvolutionResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(proto::RevertEvolutionResponse {
                success: false,
                error: e.to_string(),
            })),
        }
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
        let proto = tasks::to_proto_event(event);
        match proto.event {
            Some(proto::agent_event::Event::TextDelta(td)) => {
                assert_eq!(td.content, "hello");
            }
            other => panic!("expected TextDelta, got {other:?}"),
        }
    }

    #[test]
    fn to_proto_event_tool_execution_update() {
        let event = AgentEvent::ToolExecutionUpdate {
            id: "exec-1".to_owned(),
            message_id: "msg-1".to_owned(),
            tool_call_id: "tc-1".to_owned(),
            tool_name: "web_search".to_owned(),
            status: "running".to_owned(),
            output: None,
            error: None,
            input: None,
        };
        let proto = tasks::to_proto_event(event);
        match proto.event {
            Some(proto::agent_event::Event::ToolExecutionUpdate(teu)) => {
                assert_eq!(teu.id, "exec-1");
                assert_eq!(teu.tool_name, "web_search");
                assert_eq!(teu.status, "running");
            }
            other => panic!("expected ToolExecutionUpdate, got {other:?}"),
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
        let proto = tasks::to_proto_event(event);
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
        let proto = tasks::to_proto_event(event);
        match proto.event {
            Some(proto::agent_event::Event::Error(e)) => {
                assert_eq!(e.message, "something broke");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
