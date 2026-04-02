use sober_core::error::AppError;
use sober_core::types::{
    ConversationId, CreateTag, MessageId, MessageRepo, Tag, TagId, TagRepo, UserId,
};
use sober_db::{PgMessageRepo, PgTagRepo};
use sqlx::PgPool;
use tracing::instrument;

pub struct TagService {
    db: PgPool,
}

impl TagService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// List all tags owned by a user.
    #[instrument(level = "debug", skip(self))]
    pub async fn list_by_user(&self, user_id: UserId) -> Result<Vec<Tag>, AppError> {
        let repo = PgTagRepo::new(self.db.clone());
        repo.list_by_user(user_id).await
    }

    /// Add a tag to a conversation (creates the tag if needed).
    #[instrument(skip(self), fields(conversation.id = %conversation_id))]
    pub async fn add_to_conversation(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        name: String,
    ) -> Result<Tag, AppError> {
        super::verify_membership(&self.db, conversation_id, user_id).await?;
        let repo = PgTagRepo::new(self.db.clone());
        let tag = repo.create_or_get(CreateTag { user_id, name }).await?;
        repo.tag_conversation(conversation_id, tag.id).await?;
        Ok(tag)
    }

    /// Remove a tag from a conversation.
    #[instrument(skip(self), fields(conversation.id = %conversation_id, tag.id = %tag_id))]
    pub async fn remove_from_conversation(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        tag_id: TagId,
    ) -> Result<(), AppError> {
        super::verify_membership(&self.db, conversation_id, user_id).await?;
        PgTagRepo::new(self.db.clone())
            .untag_conversation(conversation_id, tag_id)
            .await
    }

    /// Add a tag to a message (creates the tag if needed).
    #[instrument(skip(self), fields(message.id = %message_id))]
    pub async fn add_to_message(
        &self,
        message_id: MessageId,
        user_id: UserId,
        name: String,
    ) -> Result<Tag, AppError> {
        let msg = PgMessageRepo::new(self.db.clone())
            .get_by_id(message_id)
            .await?;
        super::verify_membership(&self.db, msg.conversation_id, user_id).await?;
        let repo = PgTagRepo::new(self.db.clone());
        let tag = repo.create_or_get(CreateTag { user_id, name }).await?;
        repo.tag_message(message_id, tag.id).await?;
        Ok(tag)
    }

    /// Remove a tag from a message.
    #[instrument(skip(self), fields(message.id = %message_id, tag.id = %tag_id))]
    pub async fn remove_from_message(
        &self,
        message_id: MessageId,
        user_id: UserId,
        tag_id: TagId,
    ) -> Result<(), AppError> {
        let msg = PgMessageRepo::new(self.db.clone())
            .get_by_id(message_id)
            .await?;
        super::verify_membership(&self.db, msg.conversation_id, user_id).await?;
        PgTagRepo::new(self.db.clone())
            .untag_message(message_id, tag_id)
            .await
    }
}
