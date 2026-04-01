//! Conversation CRUD route handlers.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{
    AgentMode, ApiResponse, ConversationId, ConversationKind, ConversationRepo,
    ConversationUserRepo, ConversationUserRole, ConversationWithDetails, JobRepo,
    ListConversationsFilter, MessageId, MessageRepo, PermissionMode, PluginId, SandboxNetMode,
    TagRepo, WorkspaceRepo, WorkspaceSettingsRepo,
};
use sober_db::{
    PgConversationRepo, PgConversationUserRepo, PgJobRepo, PgMessageRepo, PgTagRepo,
    PgWorkspaceRepo, PgWorkspaceSettingsRepo,
};

use crate::state::AppState;

/// Returns the conversation routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/conversations/inbox", get(get_inbox))
        .route(
            "/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route(
            "/conversations/{id}",
            get(get_conversation)
                .patch(update_conversation)
                .delete(delete_conversation),
        )
        .route(
            "/conversations/{id}/settings",
            get(get_settings).patch(update_settings),
        )
        .route("/conversations/{id}/read", post(mark_read))
        .route("/conversations/{id}/messages", delete(clear_messages))
        .route(
            "/conversations/{id}/convert-to-group",
            post(convert_to_group),
        )
        .route("/conversations/{id}/jobs", get(list_conversation_jobs))
}

// ---------------------------------------------------------------------------
// List / Create / Get / Update / Delete conversations
// ---------------------------------------------------------------------------

/// Maximum length for auto-generated workspace names (truncated from conversation title).
const MAX_WORKSPACE_NAME_LEN: usize = 80;

/// Query parameters for `GET /conversations`.
#[derive(Deserialize)]
struct ListConversationsQuery {
    archived: Option<bool>,
    kind: Option<ConversationKind>,
    tag: Option<String>,
    search: Option<String>,
}

/// `GET /api/v1/conversations` — list conversations for the authenticated user.
async fn list_conversations(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Query(query): Query<ListConversationsQuery>,
) -> Result<ApiResponse<Vec<ConversationWithDetails>>, AppError> {
    let repo = PgConversationRepo::new(state.db.clone());
    let filter = ListConversationsFilter {
        archived: query.archived,
        kind: query.kind,
        tag: query.tag,
        search: query.search,
    };
    let conversations = repo.list_with_details(auth_user.user_id, filter).await?;

    Ok(ApiResponse::new(conversations))
}

/// Request body for `POST /conversations`.
#[derive(Deserialize)]
struct CreateConversationRequest {
    title: Option<String>,
}

/// `POST /api/v1/conversations` — create a new direct conversation.
///
/// Provisions a workspace + workspace_settings + conversation atomically.
async fn create_conversation(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Json(body): Json<CreateConversationRequest>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let ws_repo = PgWorkspaceRepo::new(state.db.clone());
    let conv_repo = PgConversationRepo::new(state.db.clone());

    // Provision workspace + settings atomically.
    let ws_name = body
        .title
        .as_deref()
        .unwrap_or("untitled")
        .chars()
        .take(MAX_WORKSPACE_NAME_LEN)
        .collect::<String>();
    let ws_root = format!(
        "{}/{}",
        state.config.workspace_root.display(),
        uuid::Uuid::now_v7()
    );
    let (workspace, _settings) = ws_repo
        .provision(auth_user.user_id, &ws_name, &ws_root)
        .await?;

    // Create conversation linked to the new workspace.
    let conversation = conv_repo
        .create(auth_user.user_id, body.title.as_deref(), Some(workspace.id))
        .await?;

    Ok(ApiResponse::new(serde_json::json!({
        "id": conversation.id.to_string(),
        "title": conversation.title,
        "workspace_id": conversation.workspace_id.map(|w| w.to_string()),
        "kind": conversation.kind,
        "agent_mode": conversation.agent_mode,
        "is_archived": conversation.is_archived,
        "unread_count": 0,
        "last_read_message_id": null,
        "tags": [],
        "created_at": conversation.created_at.to_rfc3339(),
        "updated_at": conversation.updated_at.to_rfc3339(),
    })))
}

/// `GET /api/v1/conversations/:id` — get a conversation with details.
async fn get_conversation(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<ConversationWithDetails>, AppError> {
    let conv_repo = PgConversationRepo::new(state.db.clone());
    let cu_repo = PgConversationUserRepo::new(state.db.clone());

    let conversation_id = ConversationId::from_uuid(id);

    // Verify membership.
    let cu = super::verify_membership(&state.db, conversation_id, auth_user.user_id).await?;

    let conversation = conv_repo.get_by_id(conversation_id).await?;

    // Get all users in this conversation.
    let users = cu_repo.list_by_conversation(conversation_id).await?;

    let tag_repo = PgTagRepo::new(state.db.clone());
    let tags = tag_repo.list_by_conversation(conversation_id).await?;

    // Join workspace name/path if linked.
    let (workspace_name, workspace_path) = if let Some(ws_id) = conversation.workspace_id {
        let ws_repo = PgWorkspaceRepo::new(state.db.clone());
        match ws_repo.get_by_id(ws_id).await {
            Ok(ws) => (Some(ws.name), Some(ws.root_path)),
            Err(_) => (None, None),
        }
    } else {
        (None, None)
    };

    let details = ConversationWithDetails {
        conversation,
        unread_count: cu.unread_count,
        last_read_message_id: cu.last_read_message_id,
        tags,
        users,
        workspace_name,
        workspace_path,
    };

    Ok(ApiResponse::new(details))
}

/// Request body for `PATCH /conversations/:id`.
#[derive(Deserialize)]
struct UpdateConversationRequest {
    title: Option<String>,
    archived: Option<bool>,
}

/// `PATCH /api/v1/conversations/:id` — update conversation title/archived.
async fn update_conversation(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<UpdateConversationRequest>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let repo = PgConversationRepo::new(state.db.clone());
    let conversation_id = ConversationId::from_uuid(id);

    // Verify membership.
    let _membership =
        super::verify_membership(&state.db, conversation_id, auth_user.user_id).await?;

    if let Some(ref title) = body.title {
        repo.update_title(conversation_id, title).await?;
    }
    if let Some(archived) = body.archived {
        repo.update_archived(conversation_id, archived).await?;
    }

    // Re-fetch to return current state.
    let updated = repo.get_by_id(conversation_id).await?;

    Ok(ApiResponse::new(serde_json::json!({
        "id": updated.id.to_string(),
        "title": updated.title,
        "kind": updated.kind,
        "agent_mode": updated.agent_mode,
        "is_archived": updated.is_archived,
    })))
}

/// `DELETE /api/v1/conversations/:id` — delete a conversation.
async fn delete_conversation(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let repo = PgConversationRepo::new(state.db.clone());
    let conversation_id = ConversationId::from_uuid(id);

    // Verify membership — only owner can delete.
    let membership =
        super::verify_membership(&state.db, conversation_id, auth_user.user_id).await?;
    if membership.role != ConversationUserRole::Owner {
        return Err(AppError::Forbidden);
    }

    repo.delete(conversation_id).await?;

    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}

// ---------------------------------------------------------------------------
// Settings (GET + PATCH)
// ---------------------------------------------------------------------------

/// Combined response for `GET /conversations/:id/settings`.
#[derive(serde::Serialize)]
struct SettingsResponse {
    permission_mode: PermissionMode,
    agent_mode: AgentMode,
    sandbox_profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sandbox_net_mode: Option<SandboxNetMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sandbox_allowed_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sandbox_max_execution_seconds: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sandbox_allow_spawn: Option<bool>,
    auto_snapshot: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_snapshots: Option<i32>,
    disabled_tools: Vec<String>,
    disabled_plugins: Vec<String>,
}

/// `GET /api/v1/conversations/:id/settings` — read combined settings.
async fn get_settings(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<SettingsResponse>, AppError> {
    let conversation_id = ConversationId::from_uuid(id);
    super::verify_membership(&state.db, conversation_id, auth_user.user_id).await?;

    let conv_repo = PgConversationRepo::new(state.db.clone());
    let ws_settings_repo = PgWorkspaceSettingsRepo::new(state.db.clone());

    let conversation = conv_repo.get_by_id(conversation_id).await?;

    let ws_id = conversation
        .workspace_id
        .ok_or_else(|| AppError::NotFound("workspace_settings".into()))?;

    let settings = ws_settings_repo.get_by_workspace(ws_id).await?;

    Ok(ApiResponse::new(SettingsResponse {
        permission_mode: settings.permission_mode,
        agent_mode: conversation.agent_mode,
        sandbox_profile: settings.sandbox_profile,
        sandbox_net_mode: settings.sandbox_net_mode,
        sandbox_allowed_domains: settings.sandbox_allowed_domains,
        sandbox_max_execution_seconds: settings.sandbox_max_execution_seconds,
        sandbox_allow_spawn: settings.sandbox_allow_spawn,
        auto_snapshot: settings.auto_snapshot,
        max_snapshots: settings.max_snapshots,
        disabled_tools: settings.disabled_tools,
        disabled_plugins: settings
            .disabled_plugins
            .iter()
            .map(|id| id.to_string())
            .collect(),
    }))
}

/// Request body for `PATCH /conversations/:id/settings`.
///
/// All fields optional — partial update, omitted fields unchanged.
#[derive(Deserialize)]
struct UpdateSettingsRequest {
    permission_mode: Option<PermissionMode>,
    agent_mode: Option<AgentMode>,
    sandbox_profile: Option<String>,
    sandbox_net_mode: Option<SandboxNetMode>,
    sandbox_allowed_domains: Option<Vec<String>>,
    sandbox_max_execution_seconds: Option<i32>,
    sandbox_allow_spawn: Option<bool>,
    auto_snapshot: Option<bool>,
    max_snapshots: Option<i32>,
    disabled_tools: Option<Vec<String>>,
    disabled_plugins: Option<Vec<String>>,
}

/// `PATCH /api/v1/conversations/:id/settings` — partial update.
async fn update_settings(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<UpdateSettingsRequest>,
) -> Result<ApiResponse<SettingsResponse>, AppError> {
    let conversation_id = ConversationId::from_uuid(id);
    super::verify_membership(&state.db, conversation_id, auth_user.user_id).await?;

    let conv_repo = PgConversationRepo::new(state.db.clone());
    let ws_settings_repo = PgWorkspaceSettingsRepo::new(state.db.clone());

    let conversation = conv_repo.get_by_id(conversation_id).await?;

    let ws_id = conversation
        .workspace_id
        .ok_or_else(|| AppError::NotFound("workspace_settings".into()))?;

    // Update agent_mode on conversation if provided.
    if let Some(agent_mode) = body.agent_mode {
        conv_repo
            .update_agent_mode(conversation_id, agent_mode)
            .await?;
    }

    // Load current settings and apply partial updates.
    let mut settings = ws_settings_repo.get_by_workspace(ws_id).await?;

    if let Some(mode) = body.permission_mode {
        settings.permission_mode = mode;
    }
    if let Some(profile) = body.sandbox_profile {
        settings.sandbox_profile = profile;
    }
    if let Some(net_mode) = body.sandbox_net_mode {
        settings.sandbox_net_mode = Some(net_mode);
    }
    if let Some(domains) = body.sandbox_allowed_domains {
        settings.sandbox_allowed_domains = Some(domains);
    }
    if let Some(seconds) = body.sandbox_max_execution_seconds {
        settings.sandbox_max_execution_seconds = Some(seconds);
    }
    if let Some(spawn) = body.sandbox_allow_spawn {
        settings.sandbox_allow_spawn = Some(spawn);
    }
    if let Some(snap) = body.auto_snapshot {
        settings.auto_snapshot = snap;
    }
    if let Some(max) = body.max_snapshots {
        settings.max_snapshots = Some(max);
    }
    if let Some(tools) = body.disabled_tools {
        settings.disabled_tools = tools;
    }
    if let Some(plugins) = body.disabled_plugins {
        settings.disabled_plugins = plugins
            .into_iter()
            .filter_map(|s| uuid::Uuid::parse_str(&s).ok().map(PluginId::from_uuid))
            .collect();
    }

    let updated = ws_settings_repo.upsert(&settings).await?;

    // Re-fetch conversation for current agent_mode.
    let conv = conv_repo.get_by_id(conversation_id).await?;

    Ok(ApiResponse::new(SettingsResponse {
        permission_mode: updated.permission_mode,
        agent_mode: conv.agent_mode,
        sandbox_profile: updated.sandbox_profile,
        sandbox_net_mode: updated.sandbox_net_mode,
        sandbox_allowed_domains: updated.sandbox_allowed_domains,
        sandbox_max_execution_seconds: updated.sandbox_max_execution_seconds,
        sandbox_allow_spawn: updated.sandbox_allow_spawn,
        auto_snapshot: updated.auto_snapshot,
        max_snapshots: updated.max_snapshots,
        disabled_tools: updated.disabled_tools,
        disabled_plugins: updated
            .disabled_plugins
            .iter()
            .map(|id| id.to_string())
            .collect(),
    }))
}

// ---------------------------------------------------------------------------
// Inbox / Read / Clear / Convert / Jobs
// ---------------------------------------------------------------------------

/// `GET /api/v1/conversations/inbox` — get the user's inbox conversation.
async fn get_inbox(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let repo = PgConversationRepo::new(state.db.clone());
    let conv = repo.get_inbox(auth_user.user_id).await?;

    Ok(ApiResponse::new(serde_json::json!({
        "id": conv.id.to_string(),
        "title": conv.title,
        "kind": conv.kind,
        "is_archived": conv.is_archived,
        "created_at": conv.created_at.to_rfc3339(),
        "updated_at": conv.updated_at.to_rfc3339(),
    })))
}

/// Request body for `mark_read`.
#[derive(Deserialize)]
struct MarkReadRequest {
    /// The ID of the last message the user has seen. If omitted, uses the
    /// latest message in the conversation.
    message_id: Option<uuid::Uuid>,
}

/// `POST /api/v1/conversations/:id/read` — mark conversation as read.
async fn mark_read(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    body: Option<axum::Json<MarkReadRequest>>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let conversation_id = ConversationId::from_uuid(id);

    let _membership =
        super::verify_membership(&state.db, conversation_id, auth_user.user_id).await?;

    // Resolve the message ID to mark as read.
    let message_id = if let Some(axum::Json(req)) = body {
        req.message_id
    } else {
        None
    };
    let message_id = match message_id {
        Some(mid) => MessageId::from_uuid(mid),
        None => {
            // Fall back to the latest message in the conversation.
            let msg_repo = PgMessageRepo::new(state.db.clone());
            let messages = msg_repo.list_paginated(conversation_id, None, 1).await?;
            match messages.first() {
                Some(msg) => msg.id,
                None => return Ok(ApiResponse::new(serde_json::json!({"ok": true}))),
            }
        }
    };

    let cu_repo = PgConversationUserRepo::new(state.db.clone());
    cu_repo
        .mark_read(conversation_id, auth_user.user_id, message_id)
        .await?;

    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}

/// `GET /api/v1/conversations/:id/jobs` — list jobs linked to a conversation.
async fn list_conversation_jobs(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<impl axum::response::IntoResponse, AppError> {
    let conversation_id = ConversationId::from_uuid(id);

    let _membership =
        super::verify_membership(&state.db, conversation_id, auth_user.user_id).await?;

    let job_repo = PgJobRepo::new(state.db.clone());
    let jobs = job_repo
        .list_filtered(None, None, &[], None, None, Some(id))
        .await?;
    Ok(ApiResponse::new(jobs))
}

/// Request body for `POST /conversations/:id/convert-to-group`.
#[derive(Deserialize)]
struct ConvertToGroupRequest {
    title: String,
}

/// `POST /api/v1/conversations/:id/convert-to-group` — convert a direct conversation to a group.
async fn convert_to_group(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<ConvertToGroupRequest>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let conv_repo = PgConversationRepo::new(state.db.clone());
    let conversation_id = ConversationId::from_uuid(id);

    // Verify membership — only owner can convert.
    let membership =
        super::verify_membership(&state.db, conversation_id, auth_user.user_id).await?;
    if membership.role != ConversationUserRole::Owner {
        return Err(AppError::Forbidden);
    }

    let conversation = conv_repo.get_by_id(conversation_id).await?;
    if conversation.kind != ConversationKind::Direct {
        return Err(AppError::Validation(
            "only direct conversations can be converted to group".into(),
        ));
    }

    conv_repo.convert_to_group(conversation_id).await?;
    conv_repo.update_title(conversation_id, &body.title).await?;

    let updated = conv_repo.get_by_id(conversation_id).await?;

    Ok(ApiResponse::new(serde_json::json!({
        "id": updated.id.to_string(),
        "title": updated.title,
        "kind": updated.kind,
        "agent_mode": updated.agent_mode,
        "is_archived": updated.is_archived,
    })))
}

/// `DELETE /api/v1/conversations/:id/messages` — clear all messages in a conversation.
async fn clear_messages(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let conversation_id = ConversationId::from_uuid(id);

    // Verify membership — only owner can clear messages.
    let membership =
        super::verify_membership(&state.db, conversation_id, auth_user.user_id).await?;
    if membership.role != ConversationUserRole::Owner {
        return Err(AppError::Forbidden);
    }

    let msg_repo = PgMessageRepo::new(state.db.clone());
    let cu_repo = PgConversationUserRepo::new(state.db.clone());
    msg_repo.clear_conversation(conversation_id).await?;
    cu_repo.reset_all_unread(conversation_id).await?;

    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}
