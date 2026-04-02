//! Message route handlers: pagination, deletion, and tagging.

use std::sync::Arc;

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, ContentBlock, ConversationAttachmentId, ConversationAttachmentRepo,
    ConversationId, MessageId, MessageRepo, MessageRole, Tag, TagId, TagRepo, ToolExecutionRepo,
};
use sober_db::{PgConversationAttachmentRepo, PgMessageRepo, PgTagRepo, PgToolExecutionRepo};

use crate::state::AppState;

/// Returns the message routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/conversations/{id}/messages", get(list_messages))
        .route("/messages/{id}", delete(delete_message))
        .route("/messages/{id}/tags", post(add_message_tag))
        .route("/messages/{id}/tags/{tag_id}", delete(remove_message_tag))
}

/// Query parameters for `GET /conversations/:id/messages`.
#[derive(Deserialize)]
struct PaginationParams {
    before: Option<MessageId>,
    limit: Option<i64>,
}

/// `GET /api/v1/conversations/:id/messages` — list messages with cursor pagination.
///
/// Each message includes an inline `tags` array (may be empty).
async fn list_messages(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<ConversationId>,
    Query(params): Query<PaginationParams>,
) -> Result<ApiResponse<Vec<serde_json::Value>>, AppError> {
    let msg_repo = PgMessageRepo::new(state.db.clone());
    let tag_repo = PgTagRepo::new(state.db.clone());
    let tool_exec_repo = PgToolExecutionRepo::new(state.db.clone());

    // Verify membership.
    let _membership = super::verify_membership(&state.db, id, auth_user.user_id).await?;

    let limit = params.limit.unwrap_or(50).min(100);
    let messages = msg_repo.list_paginated(id, params.before, limit).await?;

    // Batch-fetch tags for only the returned messages.
    let msg_ids: Vec<_> = messages.iter().map(|m| m.id).collect();
    let tag_pairs = tag_repo
        .list_by_message_ids(&msg_ids)
        .await
        .unwrap_or_default();
    let mut tag_map: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for (msg_id, tag) in &tag_pairs {
        tag_map
            .entry(msg_id.to_string())
            .or_default()
            .push(serde_json::to_value(tag).unwrap_or_default());
    }

    // Fetch tool executions for assistant messages.
    let mut exec_map: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for msg in &messages {
        if msg.role == MessageRole::Assistant {
            let execs: Vec<sober_core::types::ToolExecution> = tool_exec_repo
                .find_by_message(msg.id)
                .await
                .unwrap_or_default();
            if !execs.is_empty() {
                exec_map.insert(
                    msg.id.to_string(),
                    execs
                        .iter()
                        .map(|e| serde_json::to_value(e).unwrap_or_default())
                        .collect(),
                );
            }
        }
    }

    // Collect all attachment IDs from content blocks across messages.
    let mut all_attachment_ids: Vec<ConversationAttachmentId> = Vec::new();
    for msg in &messages {
        for block in &msg.content {
            let attachment_id = match block {
                ContentBlock::Image {
                    conversation_attachment_id,
                    ..
                }
                | ContentBlock::File {
                    conversation_attachment_id,
                }
                | ContentBlock::Audio {
                    conversation_attachment_id,
                }
                | ContentBlock::Video {
                    conversation_attachment_id,
                } => Some(*conversation_attachment_id),
                ContentBlock::Text { .. } => None,
            };
            if let Some(id) = attachment_id {
                all_attachment_ids.push(id);
            }
        }
    }

    // Batch-fetch attachment metadata and build a lookup map.
    let mut attachment_map: HashMap<String, serde_json::Value> = HashMap::new();
    if !all_attachment_ids.is_empty() {
        all_attachment_ids.dedup();
        let attachment_repo = PgConversationAttachmentRepo::new(state.db.clone());
        let attachments = attachment_repo
            .get_by_ids(&all_attachment_ids)
            .await
            .unwrap_or_default();
        for att in attachments {
            attachment_map.insert(
                att.id.to_string(),
                serde_json::to_value(&att).unwrap_or_default(),
            );
        }
    }

    // Attach tags, tool executions, and attachment metadata to each message.
    let response: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            let mut val = serde_json::to_value(m).unwrap_or_default();
            let tags = tag_map.remove(&m.id.to_string()).unwrap_or_default();
            if let Some(obj) = val.as_object_mut() {
                obj.insert("tags".to_string(), serde_json::Value::Array(tags));
                if m.role == MessageRole::Assistant {
                    let execs = exec_map.remove(&m.id.to_string()).unwrap_or_default();
                    obj.insert(
                        "tool_executions".to_string(),
                        serde_json::Value::Array(execs),
                    );
                }
                // Add per-message attachments map for referenced attachments.
                let mut msg_attachments = serde_json::Map::new();
                for block in &m.content {
                    let att_id = match block {
                        ContentBlock::Image {
                            conversation_attachment_id,
                            ..
                        }
                        | ContentBlock::File {
                            conversation_attachment_id,
                        }
                        | ContentBlock::Audio {
                            conversation_attachment_id,
                        }
                        | ContentBlock::Video {
                            conversation_attachment_id,
                        } => Some(conversation_attachment_id.to_string()),
                        ContentBlock::Text { .. } => None,
                    };
                    if let Some(id_str) = att_id
                        && let Some(att_val) = attachment_map.get(&id_str)
                    {
                        msg_attachments.insert(id_str, att_val.clone());
                    }
                }
                obj.insert(
                    "attachments".to_string(),
                    serde_json::Value::Object(msg_attachments),
                );
            }
            val
        })
        .collect();

    Ok(ApiResponse::new(response))
}

/// `DELETE /api/v1/messages/:id` — delete a single message.
///
/// Authorized if the caller owns the conversation OR sent the message.
async fn delete_message(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<MessageId>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let msg_repo = PgMessageRepo::new(state.db.clone());

    let msg = msg_repo.get_by_id(id).await?;

    // Verify membership.
    let membership =
        super::verify_membership(&state.db, msg.conversation_id, auth_user.user_id).await?;

    let is_owner = membership.role == sober_core::types::ConversationUserRole::Owner;
    let is_sender = msg.user_id == Some(auth_user.user_id);
    if !is_owner && !is_sender {
        return Err(AppError::NotFound("message not found".into()));
    }

    // Extract attachment IDs from content blocks before deleting.
    let attachment_ids: Vec<ConversationAttachmentId> = msg
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Image {
                conversation_attachment_id,
                ..
            }
            | ContentBlock::File {
                conversation_attachment_id,
            }
            | ContentBlock::Audio {
                conversation_attachment_id,
            }
            | ContentBlock::Video {
                conversation_attachment_id,
            } => Some(*conversation_attachment_id),
            ContentBlock::Text { .. } => None,
        })
        .collect();

    // Delete message + clean up orphaned attachments atomically.
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    PgMessageRepo::delete_tx(&mut tx, id).await?;

    if !attachment_ids.is_empty() {
        let orphaned = PgConversationAttachmentRepo::find_unreferenced_by_message_tx(
            &mut tx,
            &attachment_ids,
            msg.conversation_id,
        )
        .await?;
        for orphan_id in orphaned {
            PgConversationAttachmentRepo::delete_tx(&mut tx, orphan_id).await?;
        }
    }

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}

/// Request body for `POST /messages/:id/tags`.
#[derive(Deserialize)]
struct AddTagRequest {
    name: String,
}

/// `POST /api/v1/messages/:id/tags` — add a tag to a message.
async fn add_message_tag(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<MessageId>,
    Json(body): Json<AddTagRequest>,
) -> Result<ApiResponse<Tag>, AppError> {
    let tag = state
        .tag
        .add_to_message(id, auth_user.user_id, body.name)
        .await?;
    Ok(ApiResponse::new(tag))
}

/// `DELETE /api/v1/messages/:id/tags/:tag_id` — remove a tag from a message.
async fn remove_message_tag(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path((id, tag_id)): Path<(MessageId, TagId)>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state
        .tag
        .remove_from_message(id, auth_user.user_id, tag_id)
        .await?;
    Ok(ApiResponse::new(serde_json::json!({ "removed": true })))
}
