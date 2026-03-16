//! Agent tools for explicit memory recall and storage.
//!
//! Provides [`RecallTool`] for active memory search and [`RememberTool`]
//! for storing structured facts, preferences, and knowledge. Both wrap
//! the existing [`MemoryStore`] with LLM-facing tool interfaces.

use std::sync::Arc;

use chrono::{Duration, Utc};
use sober_core::config::MemoryConfig;
use sober_core::types::tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
use sober_core::{ScopeId, UserId};
use sober_llm::LlmEngine;
use sober_memory::bcf::ChunkType;
use sober_memory::store::{MemoryStore, StoreChunk, StoreQuery};
use uuid::Uuid;

/// Default number of results for recall queries.
const DEFAULT_RECALL_LIMIT: u64 = 10;

/// Maximum number of results for recall queries.
const MAX_RECALL_LIMIT: u64 = 20;

/// Parses a string chunk type name into its [`ChunkType`] enum value.
fn parse_chunk_type(s: &str) -> Result<ChunkType, ToolError> {
    match s {
        "fact" => Ok(ChunkType::Fact),
        "conversation" => Ok(ChunkType::Conversation),
        "preference" => Ok(ChunkType::Preference),
        "skill" => Ok(ChunkType::Skill),
        "code" => Ok(ChunkType::Code),
        "soul" => Ok(ChunkType::Soul),
        other => Err(ToolError::InvalidInput(format!(
            "unknown chunk_type '{other}'. Use: fact, conversation, preference, skill, code, soul"
        ))),
    }
}

/// Returns a display name for a [`ChunkType`].
fn chunk_type_label(ct: ChunkType) -> &'static str {
    match ct {
        ChunkType::Fact => "fact",
        ChunkType::Conversation => "conversation",
        ChunkType::Embedding => "embedding",
        ChunkType::Preference => "preference",
        ChunkType::Skill => "skill",
        ChunkType::Code => "code",
        ChunkType::Soul => "soul",
    }
}

/// Returns the default importance score for a given chunk type.
fn default_importance(ct: ChunkType) -> f64 {
    match ct {
        ChunkType::Soul => 0.9,
        ChunkType::Preference => 0.8,
        ChunkType::Fact | ChunkType::Skill => 0.7,
        ChunkType::Code => 0.6,
        ChunkType::Conversation => 0.5,
        ChunkType::Embedding => 0.5,
    }
}

/// Resolves the `owner_id` injected by the agent loop into a [`UserId`].
fn resolve_user_id(input: &serde_json::Value) -> Result<UserId, ToolError> {
    let owner_id_str = input
        .get("owner_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidInput("missing owner_id context".into()))?;
    let uuid = Uuid::parse_str(owner_id_str)
        .map_err(|e| ToolError::InvalidInput(format!("invalid owner_id: {e}")))?;
    Ok(UserId::from_uuid(uuid))
}

/// Resolves the scope from input, defaulting to the user's scope.
fn resolve_scope(input: &serde_json::Value, user_id: UserId) -> ScopeId {
    match input.get("scope").and_then(|v| v.as_str()) {
        Some("system") => ScopeId::GLOBAL,
        _ => ScopeId::from_uuid(*user_id.as_uuid()),
    }
}

// ---------------------------------------------------------------------------
// RecallTool
// ---------------------------------------------------------------------------

/// Active memory search tool. The LLM formulates a targeted query to find
/// relevant memories, bridging the semantic gap that passive retrieval misses.
pub struct RecallTool {
    memory: Arc<MemoryStore>,
    llm: Arc<dyn LlmEngine>,
    memory_config: MemoryConfig,
}

impl RecallTool {
    /// Creates a new recall tool.
    pub fn new(
        memory: Arc<MemoryStore>,
        llm: Arc<dyn LlmEngine>,
        memory_config: MemoryConfig,
    ) -> Self {
        Self {
            memory,
            llm,
            memory_config,
        }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let user_id = resolve_user_id(&input)?;
        let scope_id = resolve_scope(&input, user_id);

        let query_text = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'query'".into()))?
            .to_owned();

        let chunk_type_filter = input
            .get("chunk_type")
            .and_then(|v| v.as_str())
            .map(parse_chunk_type)
            .transpose()?;

        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_RECALL_LIMIT)
            .min(MAX_RECALL_LIMIT);

        // Embed the crafted query
        let embeddings = self
            .llm
            .embed(&[&query_text])
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("embedding failed: {e}")))?;

        let dense_vector = embeddings
            .into_iter()
            .next()
            .ok_or_else(|| ToolError::ExecutionFailed("empty embedding result".into()))?;

        let query = StoreQuery {
            dense_vector,
            query_text: query_text.clone(),
            scope_id,
            limit,
            score_threshold: None,
            chunk_type_filter: chunk_type_filter.map(u8::from),
        };

        let hits = self
            .memory
            .search(user_id, query)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("memory search failed: {e}")))?;

        if hits.is_empty() {
            return Ok(ToolOutput {
                content: format!("No memories found for query: \"{query_text}\""),
                is_error: false,
            });
        }

        // Apply retrieval boosts in the background
        for hit in &hits {
            let memory = Arc::clone(&self.memory);
            let config = self.memory_config.clone();
            let hit_point_id = hit.point_id;
            let hit_scope_id = hit.scope_id;
            tokio::spawn(async move {
                let _ = memory
                    .apply_retrieval_boost(user_id, hit_scope_id, hit_point_id, &config)
                    .await;
            });
        }

        // Format results
        let mut output = format!("Found {} memories:\n\n", hits.len());
        for (i, hit) in hits.iter().enumerate() {
            output.push_str(&format!(
                "{}. [{}] (importance: {:.2}, score: {:.3}, {})\n{}\n\n",
                i + 1,
                chunk_type_label(hit.chunk_type),
                hit.importance,
                hit.score,
                hit.created_at.format("%Y-%m-%d"),
                hit.content,
            ));
        }

        Ok(ToolOutput {
            content: output,
            is_error: false,
        })
    }
}

impl Tool for RecallTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "recall".to_owned(),
            description: "Search your long-term memory for stored facts, preferences, code, \
                skills, and conversation history. You MUST use this tool proactively:\n\
                - At the START of every new conversation to load relevant context about the user\n\
                - Whenever the user references something from the past\n\
                - Before answering any question that might depend on stored knowledge\n\
                - Before saying \"I don't know\" — always check memory first\n\
                Your passive context only includes user preferences. All facts, skills, \
                code snippets, and conversation history require explicit recall."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query, crafted for semantic relevance to what you're looking for."
                    },
                    "chunk_type": {
                        "type": "string",
                        "enum": ["fact", "conversation", "preference", "skill", "code", "soul"],
                        "description": "Filter results to a specific memory type (optional)."
                    },
                    "scope": {
                        "type": "string",
                        "enum": ["user", "system"],
                        "description": "Search scope: 'user' for personal memories (default), 'system' for global knowledge."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 10, max: 20)."
                    }
                },
                "required": ["query"]
            }),
            context_modifying: false,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

// ---------------------------------------------------------------------------
// RememberTool
// ---------------------------------------------------------------------------

/// Explicit memory storage tool. Lets the LLM store structured facts,
/// preferences, and knowledge with appropriate chunk types and importance.
pub struct RememberTool {
    memory: Arc<MemoryStore>,
    llm: Arc<dyn LlmEngine>,
    memory_config: MemoryConfig,
}

impl RememberTool {
    /// Creates a new remember tool.
    pub fn new(
        memory: Arc<MemoryStore>,
        llm: Arc<dyn LlmEngine>,
        memory_config: MemoryConfig,
    ) -> Self {
        Self {
            memory,
            llm,
            memory_config,
        }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let user_id = resolve_user_id(&input)?;
        let scope_id = ScopeId::from_uuid(*user_id.as_uuid());

        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'content'".into()))?
            .to_owned();

        let chunk_type_str = input
            .get("chunk_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'chunk_type'".into()))?;
        let chunk_type = parse_chunk_type(chunk_type_str)?;

        let importance = input
            .get("importance")
            .and_then(|v| v.as_f64())
            .unwrap_or_else(|| default_importance(chunk_type))
            .clamp(0.0, 1.0);

        // Embed the content
        let embeddings = self
            .llm
            .embed(&[&content])
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("embedding failed: {e}")))?;

        let dense_vector = embeddings
            .into_iter()
            .next()
            .ok_or_else(|| ToolError::ExecutionFailed("empty embedding result".into()))?;

        let decay_at =
            Utc::now() + Duration::days(i64::from(self.memory_config.decay_half_life_days));

        let chunk = StoreChunk {
            dense_vector,
            content: content.clone(),
            chunk_type,
            scope_id,
            source_message_id: None,
            importance,
            decay_at,
        };

        let point_id = self
            .memory
            .store(user_id, chunk)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("memory store failed: {e}")))?;

        Ok(ToolOutput {
            content: format!(
                "Stored as {} (importance: {:.1}, id: {}): \"{}\"",
                chunk_type_label(chunk_type),
                importance,
                point_id,
                if content.len() > 100 {
                    format!("{}...", &content[..100])
                } else {
                    content
                }
            ),
            is_error: false,
        })
    }
}

impl Tool for RememberTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "remember".to_owned(),
            description: "Store a fact, preference, skill, code snippet, or other knowledge \
                in memory for future recall. Use this when the user shares personal facts or \
                preferences, when you learn something useful for future conversations, when \
                the user explicitly asks you to remember something, or after extracting key \
                decisions/outcomes from a conversation."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The information to remember."
                    },
                    "chunk_type": {
                        "type": "string",
                        "enum": ["fact", "preference", "skill", "code"],
                        "description": "Type of memory: 'fact' for knowledge, 'preference' for user likes/dislikes, 'skill' for capabilities, 'code' for snippets."
                    },
                    "importance": {
                        "type": "number",
                        "description": "Importance score 0.0-1.0 (optional). Defaults vary by type: soul=0.9, preference=0.8, fact/skill=0.7, code=0.6, conversation=0.5."
                    }
                },
                "required": ["content", "chunk_type"]
            }),
            context_modifying: false,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_chunk_type_valid() {
        assert_eq!(parse_chunk_type("fact").unwrap(), ChunkType::Fact);
        assert_eq!(
            parse_chunk_type("conversation").unwrap(),
            ChunkType::Conversation
        );
        assert_eq!(
            parse_chunk_type("preference").unwrap(),
            ChunkType::Preference
        );
        assert_eq!(parse_chunk_type("skill").unwrap(), ChunkType::Skill);
        assert_eq!(parse_chunk_type("code").unwrap(), ChunkType::Code);
        assert_eq!(parse_chunk_type("soul").unwrap(), ChunkType::Soul);
    }

    #[test]
    fn parse_chunk_type_invalid() {
        assert!(parse_chunk_type("unknown").is_err());
        assert!(parse_chunk_type("").is_err());
    }

    #[test]
    fn default_importance_values() {
        assert!((default_importance(ChunkType::Soul) - 0.9).abs() < f64::EPSILON);
        assert!((default_importance(ChunkType::Preference) - 0.8).abs() < f64::EPSILON);
        assert!((default_importance(ChunkType::Fact) - 0.7).abs() < f64::EPSILON);
        assert!((default_importance(ChunkType::Skill) - 0.7).abs() < f64::EPSILON);
        assert!((default_importance(ChunkType::Code) - 0.6).abs() < f64::EPSILON);
        assert!((default_importance(ChunkType::Conversation) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn chunk_type_label_values() {
        assert_eq!(chunk_type_label(ChunkType::Fact), "fact");
        assert_eq!(chunk_type_label(ChunkType::Preference), "preference");
        assert_eq!(chunk_type_label(ChunkType::Soul), "soul");
    }
}
