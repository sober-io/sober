use sober_core::error::AppError;
use sober_core::types::{ConversationId, ConversationUser, ConversationUserRepo, UserId};
use sober_db::PgConversationUserRepo;
use sqlx::PgPool;

/// Verify the authenticated user is a member of the conversation.
pub(crate) async fn verify_membership(
    db: &PgPool,
    conversation_id: ConversationId,
    user_id: UserId,
) -> Result<ConversationUser, AppError> {
    let cu_repo = PgConversationUserRepo::new(db.clone());
    cu_repo.get(conversation_id, user_id).await
}
