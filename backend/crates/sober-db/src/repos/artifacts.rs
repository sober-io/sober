//! PostgreSQL implementation of [`ArtifactRepo`].

use sober_core::error::AppError;
use sober_core::types::{
    Artifact, ArtifactFilter, ArtifactId, ArtifactRelation, ArtifactState, CreateArtifact,
    WorkspaceId,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::ArtifactRow;

/// Column list for artifact queries.
const ARTIFACT_COLS: &str = "id, workspace_id, user_id, kind, state, title, description, \
                             storage_type, git_repo, git_ref, blob_key, inline_content, \
                             created_by, conversation_id, task_id, parent_id, \
                             reviewed_by, reviewed_at, metadata, created_at, updated_at";

/// PostgreSQL-backed artifact repository.
#[derive(Clone)]
pub struct PgArtifactRepo {
    pool: PgPool,
}

impl PgArtifactRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::ArtifactRepo for PgArtifactRepo {
    async fn create(&self, input: CreateArtifact) -> Result<Artifact, AppError> {
        let id = Uuid::now_v7();
        let query = format!(
            "INSERT INTO artifacts (id, workspace_id, user_id, kind, title, description, \
             storage_type, git_repo, git_ref, blob_key, inline_content, \
             created_by, conversation_id, task_id, parent_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15) \
             RETURNING {ARTIFACT_COLS}"
        );
        let row = sqlx::query_as::<_, ArtifactRow>(&query)
            .bind(id)
            .bind(input.workspace_id.as_uuid())
            .bind(input.user_id.as_uuid())
            .bind(input.kind)
            .bind(&input.title)
            .bind(&input.description)
            .bind(&input.storage_type)
            .bind(&input.git_repo)
            .bind(&input.git_ref)
            .bind(&input.blob_key)
            .bind(&input.inline_content)
            .bind(input.created_by.map(|u| *u.as_uuid()))
            .bind(input.conversation_id.map(|c| *c.as_uuid()))
            .bind(input.task_id)
            .bind(input.parent_id.map(|p| *p.as_uuid()))
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn get_by_id(&self, id: ArtifactId) -> Result<Artifact, AppError> {
        let query = format!("SELECT {ARTIFACT_COLS} FROM artifacts WHERE id = $1");
        let row = sqlx::query_as::<_, ArtifactRow>(&query)
            .bind(id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?
            .ok_or_else(|| AppError::NotFound("artifact".into()))?;

        Ok(row.into())
    }

    async fn list_by_workspace(
        &self,
        workspace_id: WorkspaceId,
        filter: ArtifactFilter,
    ) -> Result<Vec<Artifact>, AppError> {
        let query = format!(
            "SELECT {ARTIFACT_COLS} FROM artifacts \
             WHERE workspace_id = $1 \
               AND ($2::artifact_kind IS NULL OR kind = $2) \
               AND ($3::artifact_state IS NULL OR state = $3) \
             ORDER BY updated_at DESC"
        );
        let rows = sqlx::query_as::<_, ArtifactRow>(&query)
            .bind(workspace_id.as_uuid())
            .bind(filter.kind)
            .bind(filter.state)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_visible(
        &self,
        workspace_id: WorkspaceId,
        is_admin: bool,
    ) -> Result<Vec<Artifact>, AppError> {
        let query = if is_admin {
            format!(
                "SELECT {ARTIFACT_COLS} FROM artifacts \
                 WHERE workspace_id = $1 \
                 ORDER BY updated_at DESC"
            )
        } else {
            format!(
                "SELECT {ARTIFACT_COLS} FROM artifacts \
                 WHERE workspace_id = $1 AND kind != 'trace' \
                 ORDER BY updated_at DESC"
            )
        };
        let rows = sqlx::query_as::<_, ArtifactRow>(&query)
            .bind(workspace_id.as_uuid())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn update_state(&self, id: ArtifactId, state: ArtifactState) -> Result<(), AppError> {
        let result =
            sqlx::query("UPDATE artifacts SET state = $1, updated_at = now() WHERE id = $2")
                .bind(state)
                .bind(id.as_uuid())
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("artifact".into()));
        }

        Ok(())
    }

    async fn add_relation(
        &self,
        source: ArtifactId,
        target: ArtifactId,
        relation: ArtifactRelation,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO artifact_relations (source_id, target_id, relation) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (source_id, target_id, relation) DO NOTHING",
        )
        .bind(source.as_uuid())
        .bind(target.as_uuid())
        .bind(relation)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }
}
