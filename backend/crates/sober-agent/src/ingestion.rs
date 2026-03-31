//! Background task for embedding and storing memory extractions.

use std::sync::Arc;

use sober_core::types::ids::{ConversationId, UserId};
use sober_llm::LlmEngine;
use sober_memory::{MemoryStore, StoreChunk};
use tracing::{debug, warn};

use crate::extraction::MemoryExtraction;

/// Spawns a background task to embed and store memory extractions.
///
/// This function takes a batch of extractions, embeds them using the LLM engine,
/// and stores them in memory with appropriate decay scheduling. The operation
/// runs asynchronously in a spawned tokio task. Each extraction's `scope` field
/// determines the target scope: `"system"` → global, `"conversation"` → conversation
/// scope, default → user scope.
pub fn spawn_extraction_ingestion(
    llm: &Arc<dyn LlmEngine>,
    memory: &Arc<MemoryStore>,
    user_id: UserId,
    conversation_id: ConversationId,
    extractions: Vec<MemoryExtraction>,
    half_life_days: u32,
) {
    let llm = Arc::clone(llm);
    let memory = Arc::clone(memory);

    tokio::spawn(async move {
        let decay_at = chrono::Utc::now() + chrono::Duration::days(half_life_days as i64);

        // Batch embed all extraction contents.
        let texts: Vec<&str> = extractions.iter().map(|e| e.content.as_str()).collect();
        let vectors = match llm.embed(&texts).await {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "extraction ingestion: embedding failed, skipping");
                return;
            }
        };

        if vectors.len() != extractions.len() {
            warn!(
                "extraction ingestion: expected {} vectors, got {}",
                extractions.len(),
                vectors.len()
            );
            return;
        }

        for (extraction, dense_vector) in extractions.into_iter().zip(vectors) {
            let Some(chunk_type) = crate::extraction::parse_extraction_type(&extraction.chunk_type)
            else {
                debug!(
                    chunk_type = extraction.chunk_type,
                    "extraction ingestion: unknown chunk type, skipping"
                );
                continue;
            };

            let scope_id = match extraction.scope.as_deref() {
                Some("system") => sober_core::ScopeId::GLOBAL,
                Some("conversation") => sober_core::ScopeId::from_uuid(*conversation_id.as_uuid()),
                _ => sober_core::ScopeId::from_uuid(*user_id.as_uuid()),
            };

            let importance = crate::extraction::extraction_importance(chunk_type);

            if let Err(e) = memory
                .store(
                    user_id,
                    StoreChunk {
                        dense_vector,
                        content: extraction.content,
                        chunk_type,
                        scope_id,
                        source_message_id: None,
                        importance,
                        decay_at,
                    },
                )
                .await
            {
                warn!(error = %e, "extraction ingestion: failed to store");
            }
        }

        debug!("extraction ingestion complete for user {user_id}");
    });
}
