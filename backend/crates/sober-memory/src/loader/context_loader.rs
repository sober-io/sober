//! Context loader — combines vector search with recent messages.

use std::sync::Arc;

use sober_core::config::MemoryConfig;
use sober_core::{Message, MessageRepo, ScopeId, UserId};

use super::types::{LoadRequest, LoadedContext};
use crate::error::MemoryError;
use crate::store::{MemoryHit, MemoryStore, StoreQuery};

/// Estimates token count from text content using a simple heuristic.
fn estimate_tokens(text: &str) -> usize {
    (text.len() / 4).max(1)
}

/// Estimates token count for a message.
fn message_tokens(msg: &Message) -> usize {
    msg.token_count
        .map(|tc| tc.max(0) as usize)
        .unwrap_or_else(|| estimate_tokens(&msg.content))
}

/// Assembles context from vector search and recent messages.
///
/// Generic over `M: MessageRepo` so this crate has no dependency on
/// `sqlx` or `sober-db`.
pub struct ContextLoader<M: MessageRepo> {
    store: Arc<MemoryStore>,
    message_repo: Arc<M>,
}

impl<M: MessageRepo> ContextLoader<M> {
    /// Creates a new context loader.
    pub fn new(store: Arc<MemoryStore>, message_repo: Arc<M>) -> Self {
        Self {
            store,
            message_repo,
        }
    }

    /// Loads combined context for a query within its token budget.
    ///
    /// Priority order:
    /// 1. Recent messages (highest priority — always included first)
    /// 2. User-scope memories (personal facts)
    /// 3. System-scope memories (global knowledge)
    ///
    /// Each category is truncated to fit within the total token budget.
    pub async fn load(
        &self,
        request: LoadRequest,
        config: &MemoryConfig,
    ) -> Result<LoadedContext, MemoryError> {
        // Fetch recent messages and user memories concurrently
        let user_scope = ScopeId::from_uuid(*request.user_id.as_uuid());

        let messages_fut = self
            .message_repo
            .list_by_conversation(request.conversation_id, request.recent_message_count);

        let user_query = StoreQuery {
            dense_vector: request.query_vector.clone(),
            query_text: request.query_text.clone(),
            scope_id: user_scope,
            limit: request.hits_per_scope,
            score_threshold: None,
        };
        let user_search_fut = self.store.search(request.user_id, user_query);

        let (messages_result, user_search_result) = tokio::join!(messages_fut, user_search_fut);

        let all_messages = messages_result.map_err(|e| MemoryError::Repo(e.to_string()))?;
        let all_user_memories = user_search_result?;

        // System scope search (sequential — budget may be near-exhausted)
        let system_query = StoreQuery {
            dense_vector: request.query_vector,
            query_text: request.query_text,
            scope_id: ScopeId::GLOBAL,
            limit: request.hits_per_scope,
            score_threshold: None,
        };
        let all_system_memories = self.store.search(request.user_id, system_query).await?;

        // Assemble context within token budget
        let mut remaining = request.token_budget;

        // 1. Recent messages (highest priority)
        let mut recent_messages = Vec::new();
        for msg in &all_messages {
            let tokens = message_tokens(msg);
            if tokens <= remaining {
                recent_messages.push(msg.clone());
                remaining -= tokens;
            } else {
                tracing::warn!(
                    budget_remaining = remaining,
                    message_tokens = tokens,
                    "token budget exhausted during message inclusion"
                );
                break;
            }
        }

        // 2. User memories (sorted by score, descending — already sorted by Qdrant)
        let mut user_memories = Vec::new();
        for hit in &all_user_memories {
            let tokens = estimate_tokens(&hit.content);
            if tokens <= remaining {
                user_memories.push(hit.clone());
                remaining -= tokens;
            } else {
                break;
            }
        }

        // 3. System memories
        let mut system_memories = Vec::new();
        for hit in &all_system_memories {
            let tokens = estimate_tokens(&hit.content);
            if tokens <= remaining {
                system_memories.push(hit.clone());
                remaining -= tokens;
            } else {
                break;
            }
        }

        let estimated_tokens = request.token_budget - remaining;

        // Fire-and-forget retrieval boosts for included memories
        self.spawn_retrieval_boosts(request.user_id, &user_memories, &system_memories, config);

        Ok(LoadedContext {
            recent_messages,
            user_memories,
            system_memories,
            estimated_tokens,
        })
    }

    /// Spawns background tasks to boost importance of retrieved memories.
    fn spawn_retrieval_boosts(
        &self,
        user_id: UserId,
        user_memories: &[MemoryHit],
        system_memories: &[MemoryHit],
        config: &MemoryConfig,
    ) {
        let store = Arc::clone(&self.store);
        let boost_val = config.retrieval_boost;

        let hits: Vec<(uuid::Uuid, ScopeId)> = user_memories
            .iter()
            .chain(system_memories.iter())
            .map(|h| (h.point_id, h.scope_id))
            .collect();

        if hits.is_empty() {
            return;
        }

        let config = config.clone();
        tokio::spawn(async move {
            for (point_id, scope_id) in hits {
                if let Err(e) = store
                    .apply_retrieval_boost(user_id, scope_id, point_id, &config)
                    .await
                {
                    tracing::warn!(
                        ?point_id,
                        boost = boost_val,
                        error = %e,
                        "failed to apply retrieval boost"
                    );
                }
            }
        });
    }
}
