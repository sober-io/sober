//! Context loader — combines vector search with recent messages.

use std::sync::Arc;

use chrono::Utc;
use sober_core::config::MemoryConfig;
use sober_core::{Message, MessageRepo, ScopeId, UserId};

use super::types::{LoadRequest, LoadedContext};
use crate::error::MemoryError;
use crate::scoring;
use crate::store::ChunkType;
use crate::store::{MemoryHit, MemoryStore, StoreQuery};

/// Estimates token count from text content using a simple heuristic.
fn estimate_tokens(text: &str) -> usize {
    (text.len() / 4).max(1)
}

/// Estimates token count for a message.
fn message_tokens(msg: &Message) -> usize {
    msg.token_count
        .map(|tc| tc.max(0) as usize)
        .unwrap_or_else(|| estimate_tokens(&msg.text_content()))
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
    /// 2. Conversation-scope memories (context for this session)
    /// 3. User-scope memories (personal facts)
    /// 4. System-scope memories (global knowledge)
    ///
    /// Each category is truncated to fit within the total token budget.
    pub async fn load(
        &self,
        request: LoadRequest,
        config: &MemoryConfig,
    ) -> Result<LoadedContext, MemoryError> {
        // Ensure user + system collections exist before searching.
        tokio::try_join!(
            self.store.ensure_collection(request.user_id),
            self.store.ensure_system_collection(),
        )?;

        // Fetch recent messages, conversation memories, and user memories concurrently
        let user_scope = ScopeId::from_uuid(*request.user_id.as_uuid());
        let conv_scope = ScopeId::from_uuid(*request.conversation_id.as_uuid());

        let messages_fut = self
            .message_repo
            .list_by_conversation(request.conversation_id, request.recent_message_count);

        // Conversation scope: load ALL chunk types — conversation context
        // includes facts, decisions, and other ephemeral knowledge.
        let conv_query = StoreQuery {
            dense_vector: request.query_vector.clone(),
            query_text: request.query_text.clone(),
            scope_id: conv_scope,
            limit: request.hits_per_scope,
            score_threshold: None,
            chunk_type_filter: None,
        };
        let conv_search_fut = self.store.search(request.user_id, conv_query);

        // Passive loading fetches only Preference chunks — identity-building
        // memories that shape how the agent responds. Facts, skills, code, and
        // conversation history are retrieved on-demand via the `recall` tool.
        let user_query = StoreQuery {
            dense_vector: request.query_vector.clone(),
            query_text: request.query_text.clone(),
            scope_id: user_scope,
            limit: request.hits_per_scope,
            score_threshold: None,
            chunk_type_filter: Some(u8::from(ChunkType::Preference)),
        };
        let user_search_fut = self.store.search(request.user_id, user_query);

        let (messages_result, conv_search_result, user_search_result) =
            tokio::join!(messages_fut, conv_search_fut, user_search_fut);

        let all_messages = messages_result.map_err(|e| MemoryError::Repo(e.to_string()))?;
        let mut all_conv_memories = conv_search_result?;
        let mut all_user_memories = user_search_result?;

        // Sort by decayed importance (highest first) for token budget packing.
        let now = Utc::now();
        let half_life = config.decay_half_life_days;
        let sort_by_decayed = |hits: &mut Vec<MemoryHit>| {
            hits.sort_by(|a, b| {
                let elapsed_a = (now - a.decay_at).num_seconds().max(0) as f64 / 86400.0;
                let elapsed_b = (now - b.decay_at).num_seconds().max(0) as f64 / 86400.0;
                let da = scoring::decay(a.importance, elapsed_a, half_life);
                let db = scoring::decay(b.importance, elapsed_b, half_life);
                db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
            });
        };
        sort_by_decayed(&mut all_conv_memories);
        sort_by_decayed(&mut all_user_memories);

        // System scope search (sequential — budget may be near-exhausted)
        let system_query = StoreQuery {
            dense_vector: request.query_vector,
            query_text: request.query_text,
            scope_id: ScopeId::GLOBAL,
            limit: request.hits_per_scope,
            score_threshold: None,
            chunk_type_filter: Some(u8::from(ChunkType::Preference)),
        };
        let all_system_memories = self.store.search(request.user_id, system_query).await?;

        // Assemble context within token budget
        let mut remaining = request.token_budget;

        // 1. Recent messages (highest priority)
        //
        // Iterate newest-first so the current user message is always included.
        // Older messages are dropped first when the budget is tight.
        let mut recent_messages = Vec::new();
        for msg in all_messages.iter().rev() {
            let tokens = message_tokens(msg);
            if tokens <= remaining {
                recent_messages.push(msg.clone());
                remaining -= tokens;
            } else {
                tracing::debug!(
                    budget_remaining = remaining,
                    message_tokens = tokens,
                    "skipping older message that exceeds remaining budget"
                );
                break;
            }
        }
        // Restore chronological order (oldest first) for the LLM.
        recent_messages.reverse();

        // 2. Conversation memories (context for this session)
        let mut conversation_memories = Vec::new();
        for hit in &all_conv_memories {
            let tokens = estimate_tokens(&hit.content);
            if tokens <= remaining {
                conversation_memories.push(hit.clone());
                remaining -= tokens;
            } else {
                break;
            }
        }

        // 3. User memories (sorted by score, descending — already sorted by Qdrant)
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

        // 4. System memories
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
        self.spawn_retrieval_boosts(
            request.user_id,
            &conversation_memories,
            &user_memories,
            &system_memories,
            config,
        );

        Ok(LoadedContext {
            recent_messages,
            conversation_memories,
            user_memories,
            system_memories,
            estimated_tokens,
        })
    }

    /// Spawns background tasks to boost importance of retrieved memories.
    fn spawn_retrieval_boosts(
        &self,
        user_id: UserId,
        conversation_memories: &[MemoryHit],
        user_memories: &[MemoryHit],
        system_memories: &[MemoryHit],
        config: &MemoryConfig,
    ) {
        let store = Arc::clone(&self.store);
        let boost_val = config.retrieval_boost;

        let hits: Vec<(uuid::Uuid, ScopeId)> = conversation_memories
            .iter()
            .chain(user_memories.iter())
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
