//! PostgreSQL implementation of [`ConversationRepo`].

use sober_core::error::AppError;
use sober_core::types::{
    AgentMode, Conversation, ConversationId, ConversationKind, ConversationWithDetails,
    ListConversationsFilter, Tag, UserId, WorkspaceId,
};
use sqlx::PgPool;
use sqlx::postgres::PgConnection;
use uuid::Uuid;

use crate::rows::ConversationRow;

/// Column list for conversation queries.
const CONV_COLUMNS: &str =
    "id, user_id, title, workspace_id, kind, agent_mode, is_archived, created_at, updated_at";

/// PostgreSQL-backed conversation repository.
pub struct PgConversationRepo {
    pool: PgPool,
}

impl PgConversationRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Creates a conversation + owner membership on the given connection.
    pub async fn create_tx(
        conn: &mut PgConnection,
        user_id: UserId,
        title: Option<&str>,
        workspace_id: Option<WorkspaceId>,
    ) -> Result<Conversation, AppError> {
        let id = Uuid::now_v7();

        let row = sqlx::query_as::<_, ConversationRow>(&format!(
            "INSERT INTO conversations (id, user_id, title, workspace_id, kind) \
                 VALUES ($1, $2, $3, $4, 'direct') \
                 RETURNING {CONV_COLUMNS}"
        ))
        .bind(id)
        .bind(user_id.as_uuid())
        .bind(title)
        .bind(workspace_id.map(|w| *w.as_uuid()))
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        // Also create a conversation_users row with role = 'owner'.
        sqlx::query(
            "INSERT INTO conversation_users (conversation_id, user_id, role) \
             VALUES ($1, $2, 'owner')",
        )
        .bind(id)
        .bind(user_id.as_uuid())
        .execute(&mut *conn)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    /// Creates an inbox conversation + owner membership on the given connection.
    pub async fn create_inbox_tx(
        conn: &mut PgConnection,
        user_id: UserId,
    ) -> Result<Conversation, AppError> {
        let id = Uuid::now_v7();

        let row = sqlx::query_as::<_, ConversationRow>(&format!(
            "INSERT INTO conversations (id, user_id, kind, created_at, updated_at) \
                 VALUES ($1, $2, 'inbox', now(), now()) \
                 RETURNING {CONV_COLUMNS}"
        ))
        .bind(id)
        .bind(user_id.as_uuid())
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        // Also create a conversation_users row with role = 'owner'.
        sqlx::query(
            "INSERT INTO conversation_users (conversation_id, user_id, role) \
             VALUES ($1, $2, 'owner')",
        )
        .bind(id)
        .bind(user_id.as_uuid())
        .execute(&mut *conn)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    /// Updates conversation title on the given connection.
    pub async fn update_title_tx(
        conn: &mut PgConnection,
        id: ConversationId,
        title: &str,
    ) -> Result<(), AppError> {
        let result =
            sqlx::query("UPDATE conversations SET title = $1, updated_at = now() WHERE id = $2")
                .bind(title)
                .bind(id.as_uuid())
                .execute(&mut *conn)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("conversation".into()));
        }

        Ok(())
    }

    /// Converts a direct conversation to group on the given connection.
    pub async fn convert_to_group_tx(
        conn: &mut PgConnection,
        id: ConversationId,
    ) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE conversations SET kind = 'group', updated_at = now() \
             WHERE id = $1 AND kind = 'direct'",
        )
        .bind(id.as_uuid())
        .execute(&mut *conn)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::Validation(
                "conversation is not a direct conversation".into(),
            ));
        }

        Ok(())
    }

    /// Converts a group conversation back to direct on the given connection.
    pub async fn convert_to_direct_tx(
        conn: &mut PgConnection,
        id: ConversationId,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE conversations SET kind = 'direct', agent_mode = 'always', updated_at = now() \
             WHERE id = $1 AND kind = 'group'",
        )
        .bind(id.as_uuid())
        .execute(&mut *conn)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    /// Updates agent mode on the given connection.
    pub async fn update_agent_mode_tx(
        conn: &mut PgConnection,
        id: ConversationId,
        agent_mode: AgentMode,
    ) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE conversations SET agent_mode = $2, updated_at = now() WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(agent_mode)
        .execute(&mut *conn)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("conversation".into()));
        }

        Ok(())
    }
}

impl sober_core::types::ConversationRepo for PgConversationRepo {
    async fn create(
        &self,
        user_id: UserId,
        title: Option<&str>,
        workspace_id: Option<WorkspaceId>,
    ) -> Result<Conversation, AppError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let conv = Self::create_tx(&mut tx, user_id, title, workspace_id).await?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(conv)
    }

    async fn get_by_id(&self, id: ConversationId) -> Result<Conversation, AppError> {
        let row = sqlx::query_as::<_, ConversationRow>(&format!(
            "SELECT {CONV_COLUMNS} FROM conversations WHERE id = $1"
        ))
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("conversation".into()))?;

        Ok(row.into())
    }

    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<Conversation>, AppError> {
        let rows = sqlx::query_as::<_, ConversationRow>(&format!(
            "SELECT {CONV_COLUMNS} FROM conversations WHERE user_id = $1 \
                 ORDER BY updated_at DESC"
        ))
        .bind(user_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn update_title(&self, id: ConversationId, title: &str) -> Result<(), AppError> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        Self::update_title_tx(&mut conn, id, title).await
    }

    async fn delete(&self, id: ConversationId) -> Result<(), AppError> {
        // Check that the conversation is not an inbox — inboxes cannot be deleted.
        let kind = sqlx::query_scalar::<_, ConversationKind>(
            "SELECT kind FROM conversations WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("conversation".into()))?;

        if kind == ConversationKind::Inbox {
            return Err(AppError::Forbidden);
        }

        let result = sqlx::query("DELETE FROM conversations WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("conversation".into()));
        }

        Ok(())
    }

    async fn find_latest_by_user_and_workspace(
        &self,
        user_id: UserId,
        workspace_id: Option<WorkspaceId>,
    ) -> Result<Option<Conversation>, AppError> {
        let row = if let Some(ws_id) = workspace_id {
            sqlx::query_as::<_, ConversationRow>(&format!(
                "SELECT {CONV_COLUMNS} FROM conversations \
                     WHERE user_id = $1 AND workspace_id = $2 \
                     ORDER BY updated_at DESC LIMIT 1"
            ))
            .bind(user_id.as_uuid())
            .bind(ws_id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?
        } else {
            sqlx::query_as::<_, ConversationRow>(&format!(
                "SELECT {CONV_COLUMNS} FROM conversations WHERE user_id = $1 \
                     ORDER BY updated_at DESC LIMIT 1"
            ))
            .bind(user_id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?
        };

        Ok(row.map(Into::into))
    }

    async fn list_with_details(
        &self,
        user_id: UserId,
        filter: ListConversationsFilter,
    ) -> Result<Vec<ConversationWithDetails>, AppError> {
        // Build the main query dynamically.
        let mut qb: sqlx::QueryBuilder<'_, sqlx::Postgres> = sqlx::QueryBuilder::new(
            "SELECT c.id, c.user_id, c.title, c.workspace_id, c.kind, c.agent_mode, c.is_archived, \
             c.created_at, c.updated_at, \
             COALESCE(cu.unread_count, 0) AS unread_count, \
             cu.last_read_message_id, \
             w.name AS workspace_name, w.root_path AS workspace_path \
             FROM conversations c \
             LEFT JOIN workspaces w ON w.id = c.workspace_id \
             LEFT JOIN conversation_users cu ON cu.conversation_id = c.id AND cu.user_id = ",
        );
        qb.push_bind(*user_id.as_uuid());

        // Tag filter requires a join.
        if filter.tag.is_some() {
            qb.push(
                " INNER JOIN conversation_tags ct ON ct.conversation_id = c.id \
                 INNER JOIN tags t ON t.id = ct.tag_id AND t.user_id = ",
            );
            qb.push_bind(*user_id.as_uuid());
        }

        qb.push(" WHERE cu.user_id IS NOT NULL");

        if let Some(archived) = filter.archived {
            qb.push(" AND c.is_archived = ");
            qb.push_bind(archived);
        }

        if let Some(kind) = filter.kind {
            qb.push(" AND c.kind = ");
            qb.push_bind(kind);
        }

        if let Some(ref tag_name) = filter.tag {
            qb.push(" AND t.name = ");
            qb.push_bind(tag_name);
        }

        if let Some(ref search) = filter.search {
            qb.push(" AND c.title ILIKE ");
            qb.push_bind(format!("%{search}%"));
        }

        qb.push(" ORDER BY c.updated_at DESC");

        let rows = qb
            .build_query_as::<crate::rows::ConversationWithUnreadRow>()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if rows.is_empty() {
            return Ok(Vec::new());
        }

        // Collect conversation IDs for batch tag fetch.
        let conv_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();

        // Fetch tags for all returned conversations.
        let tag_rows = sqlx::query_as::<_, TagWithConversationId>(
            "SELECT ct.conversation_id, t.id, t.user_id, t.name, t.color, t.created_at \
             FROM conversation_tags ct \
             INNER JOIN tags t ON t.id = ct.tag_id \
             WHERE ct.conversation_id = ANY($1)",
        )
        .bind(&conv_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        // Group tags by conversation_id.
        let mut tags_by_conv: std::collections::HashMap<Uuid, Vec<Tag>> =
            std::collections::HashMap::new();
        for tr in tag_rows {
            let tag = Tag {
                id: sober_core::types::TagId::from_uuid(tr.id),
                user_id: sober_core::types::UserId::from_uuid(tr.user_id),
                name: tr.name,
                color: tr.color,
                created_at: tr.created_at,
            };
            tags_by_conv
                .entry(tr.conversation_id)
                .or_default()
                .push(tag);
        }

        // Build results.
        let results = rows
            .into_iter()
            .map(|r| {
                let conv_id = r.id;
                ConversationWithDetails {
                    conversation: Conversation {
                        id: sober_core::types::ConversationId::from_uuid(r.id),
                        user_id: sober_core::types::UserId::from_uuid(r.user_id),
                        title: r.title,
                        workspace_id: r
                            .workspace_id
                            .map(sober_core::types::WorkspaceId::from_uuid),
                        kind: r.kind,
                        agent_mode: r.agent_mode,
                        is_archived: r.is_archived,
                        created_at: r.created_at,
                        updated_at: r.updated_at,
                    },
                    unread_count: r.unread_count,
                    last_read_message_id: r
                        .last_read_message_id
                        .map(sober_core::types::MessageId::from_uuid),
                    tags: tags_by_conv.remove(&conv_id).unwrap_or_default(),
                    users: Vec::new(),
                    workspace_name: r.workspace_name,
                    workspace_path: r.workspace_path,
                }
            })
            .collect();

        Ok(results)
    }

    async fn get_inbox(&self, user_id: UserId) -> Result<Conversation, AppError> {
        let row = sqlx::query_as::<_, ConversationRow>(&format!(
            "SELECT {CONV_COLUMNS} FROM conversations \
                 WHERE user_id = $1 AND kind = 'inbox'"
        ))
        .bind(user_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("inbox".into()))?;

        Ok(row.into())
    }

    async fn create_inbox(&self, user_id: UserId) -> Result<Conversation, AppError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let conv = Self::create_inbox_tx(&mut tx, user_id).await?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(conv)
    }

    async fn update_archived(&self, id: ConversationId, archived: bool) -> Result<(), AppError> {
        // NOTE: per design, archive/unarchive does NOT update updated_at.
        let result = sqlx::query("UPDATE conversations SET is_archived = $2 WHERE id = $1")
            .bind(id.as_uuid())
            .bind(archived)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("conversation".into()));
        }

        Ok(())
    }

    async fn update_workspace(
        &self,
        id: ConversationId,
        workspace_id: Option<WorkspaceId>,
    ) -> Result<(), AppError> {
        let result = sqlx::query("UPDATE conversations SET workspace_id = $2 WHERE id = $1")
            .bind(id.as_uuid())
            .bind(workspace_id.map(|w| *w.as_uuid()))
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("conversation".into()));
        }

        Ok(())
    }

    async fn update_agent_mode(
        &self,
        id: ConversationId,
        agent_mode: AgentMode,
    ) -> Result<(), AppError> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        Self::update_agent_mode_tx(&mut conn, id, agent_mode).await
    }

    async fn convert_to_group(&self, id: ConversationId) -> Result<(), AppError> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        Self::convert_to_group_tx(&mut conn, id).await
    }

    async fn convert_to_direct(&self, id: ConversationId) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE conversations SET kind = 'direct', agent_mode = 'always', updated_at = now() \
             WHERE id = $1 AND kind = 'group'",
        )
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn list_recent(&self, limit: i64) -> Result<Vec<Conversation>, AppError> {
        let rows = sqlx::query_as::<_, ConversationRow>(&format!(
            "SELECT {CONV_COLUMNS} FROM conversations \
                 WHERE kind != 'inbox' \
                 ORDER BY updated_at DESC LIMIT $1"
        ))
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn filter_existing_ids(
        &self,
        ids: &[ConversationId],
    ) -> Result<Vec<ConversationId>, AppError> {
        let uuids: Vec<uuid::Uuid> = ids.iter().map(|id| *id.as_uuid()).collect();
        let rows =
            sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM conversations WHERE id = ANY($1)")
                .bind(&uuids)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(ConversationId::from_uuid).collect())
    }
}

/// Helper row type for the tag + conversation_id join.
#[derive(sqlx::FromRow)]
struct TagWithConversationId {
    conversation_id: Uuid,
    id: Uuid,
    user_id: Uuid,
    name: String,
    color: String,
    created_at: chrono::DateTime<chrono::Utc>,
}
