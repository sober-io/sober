//! gRPC service implementation for the agent.
//!
//! Bridges between the tonic-generated proto types and the [`Agent`] struct.

use std::sync::Arc;

use sober_core::types::AgentRepos;
use sober_core::types::JobPayload;
use sober_core::types::access::{CallerContext, TriggerKind};
use sober_core::types::ids::{ConversationId, PluginId, UserId, WorkspaceId};
use sober_core::types::repo::{ConversationRepo, PluginRepo, WorkspaceRepo};
use sober_plugin::PluginManager;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{error, info, warn};

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
        request: Request<proto::ListSkillsRequest>,
    ) -> Result<Response<proto::ListSkillsResponse>, Status> {
        use sober_core::types::enums::{PluginKind, PluginScope, PluginStatus};
        use sober_core::types::input::PluginFilter;
        use std::collections::HashSet;

        let req = request.into_inner();

        // Resolve workspace_id from conversation_id for workspace-scoped filtering.
        let workspace_id = if let Some(conv_id_str) = req.conversation_id.as_deref() {
            if let Ok(uuid) = conv_id_str.parse::<uuid::Uuid>() {
                let conv_id = ConversationId::from_uuid(uuid);
                self.agent
                    .repos()
                    .conversations()
                    .get_by_id(conv_id)
                    .await
                    .ok()
                    .and_then(|c| c.workspace_id)
            } else {
                None
            }
        } else {
            None
        };

        // Database is the source of truth. Return enabled skill plugins.
        let filter = PluginFilter {
            kind: Some(PluginKind::Skill),
            status: Some(PluginStatus::Enabled),
            ..Default::default()
        };

        let plugins = self
            .agent
            .repos()
            .plugins()
            .list(filter)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        // Filter: include system + user scoped skills always,
        // workspace-scoped only if they match the current workspace.
        // Deduplicate by name (first occurrence wins).
        let mut seen = HashSet::new();
        let skills = plugins
            .into_iter()
            .filter(|p| match p.scope {
                PluginScope::System | PluginScope::User => true,
                PluginScope::Workspace => match (p.workspace_id, workspace_id) {
                    (Some(pw), Some(cw)) => pw == cw,
                    _ => false,
                },
            })
            .filter(|p| seen.insert(p.name.clone()))
            .map(|p| proto::SkillInfo {
                name: p.name,
                description: p.description.unwrap_or_default(),
            })
            .collect();

        Ok(Response::new(proto::ListSkillsResponse { skills }))
    }

    async fn reload_skills(
        &self,
        request: Request<proto::ReloadSkillsRequest>,
    ) -> Result<Response<proto::ReloadSkillsResponse>, Status> {
        use sober_core::types::enums::{PluginKind, PluginScope, PluginStatus};
        use sober_core::types::input::PluginFilter;
        use std::collections::HashSet;

        let req = request.into_inner();

        // Invalidate skill cache so next tools_for_turn re-discovers from disk.
        self.plugin_manager.skill_loader().invalidate_cache();

        // Resolve workspace context from conversation_id.
        let (workspace_id, workspace_path) =
            if let Some(conv_id_str) = req.conversation_id.as_deref() {
                if let Ok(uuid) = conv_id_str.parse::<uuid::Uuid>() {
                    let conv_id = ConversationId::from_uuid(uuid);
                    let ws_id = self
                        .agent
                        .repos()
                        .conversations()
                        .get_by_id(conv_id)
                        .await
                        .ok()
                        .and_then(|c| c.workspace_id);
                    let ws_dir = self
                        .agent
                        .resolve_workspace_dir(conv_id)
                        .await
                        .unwrap_or_default();
                    (ws_id, ws_dir)
                } else {
                    (None, std::path::PathBuf::new())
                }
            } else {
                (None, std::path::PathBuf::new())
            };

        // Re-sync filesystem skills into the DB via the plugin manager.
        let user_home = sober_workspace::user_home_dir();
        let _ = self
            .plugin_manager
            .tools_for_turn(
                UserId::from_uuid(uuid::Uuid::nil()),
                &user_home,
                &workspace_path,
                workspace_id,
                None,
            )
            .await;

        // Return enabled skills from the database, filtered by workspace scope.
        let filter = PluginFilter {
            kind: Some(PluginKind::Skill),
            status: Some(PluginStatus::Enabled),
            ..Default::default()
        };

        let plugins = self
            .agent
            .repos()
            .plugins()
            .list(filter)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let mut seen = HashSet::new();
        let skills = plugins
            .into_iter()
            .filter(|p| match p.scope {
                PluginScope::System | PluginScope::User => true,
                PluginScope::Workspace => match (p.workspace_id, workspace_id) {
                    (Some(pw), Some(cw)) => pw == cw,
                    _ => false,
                },
            })
            .filter(|p| seen.insert(p.name.clone()))
            .map(|p| proto::SkillInfo {
                name: p.name,
                description: p.description.unwrap_or_default(),
            })
            .collect();

        Ok(Response::new(proto::ReloadSkillsResponse { skills }))
    }

    async fn list_plugins(
        &self,
        request: Request<proto::ListPluginsRequest>,
    ) -> Result<Response<proto::ListPluginsResponse>, Status> {
        let req = request.into_inner();

        let kind_filter = req
            .kind
            .map(|k| parse_plugin_kind(&k))
            .transpose()
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        let status_filter = req
            .status
            .map(|s| parse_plugin_status(&s))
            .transpose()
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        let filter = sober_core::types::PluginFilter {
            kind: kind_filter,
            status: status_filter,
            ..Default::default()
        };

        let plugins = self
            .agent
            .repos()
            .plugins()
            .list(filter)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let plugin_infos = plugins.iter().map(plugin_to_proto).collect();

        Ok(Response::new(proto::ListPluginsResponse {
            plugins: plugin_infos,
        }))
    }

    async fn install_plugin(
        &self,
        request: Request<proto::InstallPluginRequest>,
    ) -> Result<Response<proto::InstallPluginResponse>, Status> {
        let req = request.into_inner();

        let kind =
            parse_plugin_kind(&req.kind).map_err(|e| Status::invalid_argument(e.to_string()))?;

        let config: serde_json::Value = serde_json::from_str(&req.config)
            .map_err(|e| Status::invalid_argument(format!("invalid config JSON: {e}")))?;

        let input = sober_core::types::CreatePlugin {
            name: req.name,
            kind,
            version: req.version,
            description: req.description,
            origin: sober_core::types::PluginOrigin::User,
            scope: sober_core::types::PluginScope::System,
            owner_id: None,
            workspace_id: None,
            status: sober_core::types::PluginStatus::Enabled,
            config,
            installed_by: None,
        };

        let plugin = self
            .agent
            .repos()
            .plugins()
            .create(input)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        info!(plugin_id = %plugin.id, name = %plugin.name, "plugin installed");

        Ok(Response::new(proto::InstallPluginResponse {
            plugin: Some(plugin_to_proto(&plugin)),
        }))
    }

    async fn uninstall_plugin(
        &self,
        request: Request<proto::UninstallPluginRequest>,
    ) -> Result<Response<proto::UninstallPluginResponse>, Status> {
        let req = request.into_inner();

        let plugin_id = req
            .plugin_id
            .parse::<uuid::Uuid>()
            .map(PluginId::from_uuid)
            .map_err(|_| Status::invalid_argument("invalid plugin_id"))?;

        self.agent
            .repos()
            .plugins()
            .delete(plugin_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        info!(%plugin_id, "plugin uninstalled");

        Ok(Response::new(proto::UninstallPluginResponse {}))
    }

    async fn enable_plugin(
        &self,
        request: Request<proto::EnablePluginRequest>,
    ) -> Result<Response<proto::EnablePluginResponse>, Status> {
        let req = request.into_inner();

        let plugin_id = req
            .plugin_id
            .parse::<uuid::Uuid>()
            .map(PluginId::from_uuid)
            .map_err(|_| Status::invalid_argument("invalid plugin_id"))?;

        self.agent
            .repos()
            .plugins()
            .update_status(plugin_id, sober_core::types::PluginStatus::Enabled)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        info!(%plugin_id, "plugin enabled");

        Ok(Response::new(proto::EnablePluginResponse {}))
    }

    async fn disable_plugin(
        &self,
        request: Request<proto::DisablePluginRequest>,
    ) -> Result<Response<proto::DisablePluginResponse>, Status> {
        let req = request.into_inner();

        let plugin_id = req
            .plugin_id
            .parse::<uuid::Uuid>()
            .map(PluginId::from_uuid)
            .map_err(|_| Status::invalid_argument("invalid plugin_id"))?;

        self.agent
            .repos()
            .plugins()
            .update_status(plugin_id, sober_core::types::PluginStatus::Disabled)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        info!(%plugin_id, "plugin disabled");

        Ok(Response::new(proto::DisablePluginResponse {}))
    }

    async fn import_plugins(
        &self,
        request: Request<proto::ImportPluginsRequest>,
    ) -> Result<Response<proto::ImportPluginsResponse>, Status> {
        let req = request.into_inner();

        let mcp_config: std::collections::HashMap<String, McpServerEntry> =
            serde_json::from_str(&req.mcp_servers_json)
                .map_err(|e| Status::invalid_argument(format!("invalid mcpServers JSON: {e}")))?;

        let mut imported = Vec::new();

        for (name, entry) in &mcp_config {
            let config = serde_json::json!({
                "command": entry.command,
                "args": entry.args,
                "env": entry.env,
            });

            let input = sober_core::types::CreatePlugin {
                name: name.clone(),
                kind: sober_core::types::PluginKind::Mcp,
                version: None,
                description: None,
                origin: sober_core::types::PluginOrigin::User,
                scope: sober_core::types::PluginScope::System,
                owner_id: None,
                workspace_id: None,
                status: sober_core::types::PluginStatus::Enabled,
                config,
                installed_by: None,
            };

            match self.agent.repos().plugins().create(input).await {
                Ok(plugin) => {
                    info!(plugin_id = %plugin.id, name = %plugin.name, "MCP plugin imported");
                    imported.push(plugin);
                }
                Err(e) => {
                    warn!(name = %name, error = %e, "failed to import MCP plugin");
                }
            }
        }

        let plugin_infos = imported.iter().map(plugin_to_proto).collect();

        Ok(Response::new(proto::ImportPluginsResponse {
            imported_count: imported.len() as u32,
            plugins: plugin_infos,
        }))
    }

    async fn reload_plugins(
        &self,
        _request: Request<proto::ReloadPluginsRequest>,
    ) -> Result<Response<proto::ReloadPluginsResponse>, Status> {
        let filter = sober_core::types::PluginFilter {
            status: Some(sober_core::types::PluginStatus::Enabled),
            ..Default::default()
        };

        let active_plugins = self
            .agent
            .repos()
            .plugins()
            .list(filter)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        info!(count = active_plugins.len(), "plugins reloaded");

        Ok(Response::new(proto::ReloadPluginsResponse {
            active_count: active_plugins.len() as u32,
        }))
    }

    async fn change_plugin_scope(
        &self,
        request: Request<proto::ChangePluginScopeRequest>,
    ) -> Result<Response<proto::ChangePluginScopeResponse>, Status> {
        let req = request.into_inner();

        let plugin_id = req
            .plugin_id
            .parse::<uuid::Uuid>()
            .map(PluginId::from_uuid)
            .map_err(|_| Status::invalid_argument("invalid plugin_id"))?;

        let new_scope = match req.new_scope.as_str() {
            "system" => sober_core::types::PluginScope::System,
            "user" => sober_core::types::PluginScope::User,
            "workspace" => sober_core::types::PluginScope::Workspace,
            other => return Err(Status::invalid_argument(format!("unknown scope: {other}"))),
        };

        let new_workspace_id = req
            .workspace_id
            .map(|s| {
                s.parse::<uuid::Uuid>()
                    .map(WorkspaceId::from_uuid)
                    .map_err(|_| Status::invalid_argument("invalid workspace_id"))
            })
            .transpose()?;

        // Fetch the plugin.
        let plugin = self
            .agent
            .repos()
            .plugins()
            .get_by_id(plugin_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        // Move files on disk for skill plugins.
        if plugin.kind == sober_core::types::PluginKind::Skill
            && let Some(src_path) = plugin.config.get("path").and_then(|v| v.as_str())
        {
            let src = std::path::Path::new(src_path);

            // Determine destination directory.
            let user_home = sober_workspace::user_home_dir();
            let dest_dir = match new_scope {
                sober_core::types::PluginScope::User => user_home.join(".sober").join("skills"),
                sober_core::types::PluginScope::Workspace => {
                    // Resolve workspace root from workspace_id.
                    let ws_id = new_workspace_id.ok_or_else(|| {
                        Status::invalid_argument(
                            "workspace_id required when moving to workspace scope",
                        )
                    })?;
                    let ws = self
                        .agent
                        .repos()
                        .workspaces()
                        .get_by_id(ws_id)
                        .await
                        .map_err(|e| Status::internal(e.to_string()))?;
                    std::path::PathBuf::from(&ws.root_path)
                        .join(".sober")
                        .join("skills")
                }
                sober_core::types::PluginScope::System => {
                    return Err(Status::invalid_argument(
                        "cannot move skill files to system scope (system skills are compiled in)",
                    ));
                }
            };

            if src.exists() {
                // Determine if src is a file (skill.md) or directory (skill-name/).
                let (dest_path, new_config_path) = if src.is_dir() {
                    let dir_name = src
                        .file_name()
                        .ok_or_else(|| Status::internal("invalid source path"))?;
                    let dest = dest_dir.join(dir_name);
                    let config_path = dest.join("SKILL.md").to_string_lossy().to_string();
                    (dest, config_path)
                } else {
                    let file_name = src
                        .file_name()
                        .ok_or_else(|| Status::internal("invalid source path"))?;
                    let dest = dest_dir.join(file_name);
                    let config_path = dest.to_string_lossy().to_string();
                    (dest, config_path)
                };

                // Create destination directory.
                tokio::fs::create_dir_all(&dest_dir).await.map_err(|e| {
                    Status::internal(format!("failed to create destination directory: {e}"))
                })?;

                // Move: copy source to dest, then remove source.
                // Use the parent directory for directory moves.
                if src.is_dir() {
                    copy_dir_recursive(src, &dest_path).await.map_err(|e| {
                        Status::internal(format!("failed to copy skill directory: {e}"))
                    })?;
                    tokio::fs::remove_dir_all(src).await.map_err(|e| {
                        Status::internal(format!("failed to remove source directory: {e}"))
                    })?;
                } else {
                    tokio::fs::copy(src, &dest_path)
                        .await
                        .map_err(|e| Status::internal(format!("failed to copy skill file: {e}")))?;
                    tokio::fs::remove_file(src).await.map_err(|e| {
                        Status::internal(format!("failed to remove source file: {e}"))
                    })?;
                }

                // Update config with new path.
                let mut config = plugin.config.clone();
                config["path"] = serde_json::Value::String(new_config_path);
                config["source"] = serde_json::Value::String(format!("{new_scope:?}"));
                self.agent
                    .repos()
                    .plugins()
                    .update_config(plugin_id, config)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;
            }
        }
        // Update scope and workspace_id in DB.
        self.agent
            .repos()
            .plugins()
            .update_scope(plugin_id, new_scope)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        // Invalidate skill cache so next load picks up the new location.
        self.plugin_manager.skill_loader().invalidate_cache();

        // Re-fetch and return.
        let updated = self
            .agent
            .repos()
            .plugins()
            .get_by_id(plugin_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        info!(
            %plugin_id,
            name = %updated.name,
            scope = ?new_scope,
            "plugin scope changed"
        );

        Ok(Response::new(proto::ChangePluginScopeResponse {
            plugin: Some(plugin_to_proto(&updated)),
        }))
    }
}

/// JSON structure for an MCP server entry from `.mcp.json` format.
#[derive(Debug, serde::Deserialize)]
struct McpServerEntry {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: std::collections::HashMap<String, String>,
}

/// Converts a domain [`Plugin`] to the proto [`PluginInfo`] representation.
fn plugin_to_proto(plugin: &sober_core::types::Plugin) -> proto::PluginInfo {
    proto::PluginInfo {
        id: plugin.id.to_string(),
        name: plugin.name.clone(),
        kind: format!("{:?}", plugin.kind).to_lowercase(),
        version: plugin.version.clone().unwrap_or_default(),
        description: plugin.description.clone().unwrap_or_default(),
        status: format!("{:?}", plugin.status).to_lowercase(),
        config: plugin.config.to_string(),
        installed_at: plugin.installed_at.to_rfc3339(),
        scope: format!("{:?}", plugin.scope).to_lowercase(),
    }
}

/// Parses a string into a [`PluginKind`] enum variant.
fn parse_plugin_kind(s: &str) -> Result<sober_core::types::PluginKind, String> {
    match s.to_lowercase().as_str() {
        "mcp" => Ok(sober_core::types::PluginKind::Mcp),
        "skill" => Ok(sober_core::types::PluginKind::Skill),
        "wasm" => Ok(sober_core::types::PluginKind::Wasm),
        other => Err(format!("unknown plugin kind: {other}")),
    }
}

/// Parses a string into a [`PluginStatus`] enum variant.
fn parse_plugin_status(s: &str) -> Result<sober_core::types::PluginStatus, String> {
    match s.to_lowercase().as_str() {
        "enabled" => Ok(sober_core::types::PluginStatus::Enabled),
        "disabled" => Ok(sober_core::types::PluginStatus::Disabled),
        "failed" => Ok(sober_core::types::PluginStatus::Failed),
        other => Err(format!("unknown plugin status: {other}")),
    }
}

/// Recursively copies a directory and its contents.
async fn copy_dir_recursive(
    src: &std::path::Path,
    dst: &std::path::Path,
) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type().await?.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    Ok(())
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

    #[test]
    fn parse_plugin_kind_valid() {
        assert_eq!(
            parse_plugin_kind("mcp").unwrap(),
            sober_core::types::PluginKind::Mcp
        );
        assert_eq!(
            parse_plugin_kind("MCP").unwrap(),
            sober_core::types::PluginKind::Mcp
        );
        assert_eq!(
            parse_plugin_kind("skill").unwrap(),
            sober_core::types::PluginKind::Skill
        );
        assert_eq!(
            parse_plugin_kind("wasm").unwrap(),
            sober_core::types::PluginKind::Wasm
        );
    }

    #[test]
    fn parse_plugin_kind_invalid() {
        assert!(parse_plugin_kind("unknown").is_err());
        assert!(parse_plugin_kind("").is_err());
    }

    #[test]
    fn parse_plugin_status_valid() {
        assert_eq!(
            parse_plugin_status("enabled").unwrap(),
            sober_core::types::PluginStatus::Enabled
        );
        assert_eq!(
            parse_plugin_status("DISABLED").unwrap(),
            sober_core::types::PluginStatus::Disabled
        );
        assert_eq!(
            parse_plugin_status("failed").unwrap(),
            sober_core::types::PluginStatus::Failed
        );
    }

    #[test]
    fn parse_plugin_status_invalid() {
        assert!(parse_plugin_status("unknown").is_err());
        assert!(parse_plugin_status("").is_err());
    }

    #[test]
    fn plugin_to_proto_conversion() {
        let plugin = sober_core::types::Plugin {
            id: PluginId::new(),
            name: "test-plugin".to_owned(),
            kind: sober_core::types::PluginKind::Mcp,
            version: Some("1.0.0".to_owned()),
            description: Some("A test plugin".to_owned()),
            origin: sober_core::types::PluginOrigin::User,
            scope: sober_core::types::PluginScope::System,
            owner_id: None,
            workspace_id: None,
            status: sober_core::types::PluginStatus::Enabled,
            config: serde_json::json!({"command": "test"}),
            installed_by: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let proto_info = plugin_to_proto(&plugin);

        assert_eq!(proto_info.id, plugin.id.to_string());
        assert_eq!(proto_info.name, "test-plugin");
        assert_eq!(proto_info.kind, "mcp");
        assert_eq!(proto_info.version, "1.0.0");
        assert_eq!(proto_info.description, "A test plugin");
        assert_eq!(proto_info.status, "enabled");
        assert!(proto_info.config.contains("command"));
        assert!(!proto_info.installed_at.is_empty());
    }

    #[test]
    fn plugin_to_proto_defaults() {
        let plugin = sober_core::types::Plugin {
            id: PluginId::new(),
            name: "minimal".to_owned(),
            kind: sober_core::types::PluginKind::Wasm,
            version: None,
            description: None,
            origin: sober_core::types::PluginOrigin::Agent,
            scope: sober_core::types::PluginScope::User,
            owner_id: None,
            workspace_id: None,
            status: sober_core::types::PluginStatus::Disabled,
            config: serde_json::json!({}),
            installed_by: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let proto_info = plugin_to_proto(&plugin);

        assert_eq!(proto_info.kind, "wasm");
        assert_eq!(proto_info.version, "");
        assert_eq!(proto_info.description, "");
        assert_eq!(proto_info.status, "disabled");
    }

    #[test]
    fn mcp_server_entry_deserialization() {
        let json = r#"{"test-server": {"command": "node", "args": ["server.js"], "env": {"PORT": "3000"}}}"#;
        let parsed: std::collections::HashMap<String, McpServerEntry> =
            serde_json::from_str(json).expect("valid JSON");

        assert_eq!(parsed.len(), 1);
        let entry = parsed.get("test-server").expect("entry exists");
        assert_eq!(entry.command, "node");
        assert_eq!(entry.args, vec!["server.js"]);
        assert_eq!(entry.env.get("PORT").unwrap(), "3000");
    }

    #[test]
    fn mcp_server_entry_defaults() {
        let json = r#"{"minimal": {"command": "echo"}}"#;
        let parsed: std::collections::HashMap<String, McpServerEntry> =
            serde_json::from_str(json).expect("valid JSON");

        let entry = parsed.get("minimal").expect("entry exists");
        assert_eq!(entry.command, "echo");
        assert!(entry.args.is_empty());
        assert!(entry.env.is_empty());
    }
}
