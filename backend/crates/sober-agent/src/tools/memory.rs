//! Agent tools for explicit memory recall and storage.
//!
//! Provides [`RecallTool`] for active memory search and [`RememberTool`]
//! for storing structured facts, preferences, and knowledge. Both wrap
//! the existing [`MemoryStore`] with LLM-facing tool interfaces.

use std::sync::Arc;

use chrono::{Duration, Utc};
use sober_core::config::MemoryConfig;
use sober_core::types::ids::ConversationId;
use sober_core::types::repo::MessageRepo;
use sober_core::types::tool::{
    BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput, ToolVisibility,
};
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

/// Maximum length for recall queries.
const MAX_QUERY_LENGTH: usize = 256;

/// Active memory search tool. The LLM formulates a targeted query to find
/// relevant memories or search past conversation messages, bridging the
/// semantic gap that passive retrieval misses.
pub struct RecallTool<M: MessageRepo> {
    memory: Arc<MemoryStore>,
    llm: Arc<dyn LlmEngine>,
    memory_config: MemoryConfig,
    messages: Arc<M>,
}

impl<M: MessageRepo> RecallTool<M> {
    /// Creates a new recall tool.
    pub fn new(
        memory: Arc<MemoryStore>,
        llm: Arc<dyn LlmEngine>,
        memory_config: MemoryConfig,
        messages: Arc<M>,
    ) -> Self {
        Self {
            memory,
            llm,
            memory_config,
            messages,
        }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let user_id = resolve_user_id(&input)?;

        let query_text = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'query'".into()))?
            .to_owned();

        if query_text.is_empty() {
            return Err(ToolError::InvalidInput("query must not be empty".into()));
        }

        if query_text.len() > MAX_QUERY_LENGTH {
            return Err(ToolError::InvalidInput(
                "query too long (max 256 characters)".into(),
            ));
        }

        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_RECALL_LIMIT)
            .min(MAX_RECALL_LIMIT);

        let source = input
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("memory");

        match source {
            "memory" => {
                self.search_memory(user_id, &query_text, &input, limit)
                    .await
            }
            "conversations" => {
                self.search_conversations(user_id, &query_text, &input, limit as i64)
                    .await
            }
            _ => Err(ToolError::InvalidInput(
                "unknown source: use 'memory' or 'conversations'".into(),
            )),
        }
    }

    /// Searches vector memory (Qdrant) for stored knowledge chunks.
    async fn search_memory(
        &self,
        user_id: UserId,
        query_text: &str,
        input: &serde_json::Value,
        limit: u64,
    ) -> Result<ToolOutput, ToolError> {
        let scope_id = resolve_scope(input, user_id);

        let chunk_type_filter = input
            .get("chunk_type")
            .and_then(|v| v.as_str())
            .map(parse_chunk_type)
            .transpose()?;

        // Embed the crafted query
        let embeddings = self
            .llm
            .embed(&[query_text])
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("embedding failed: {e}")))?;

        let dense_vector = embeddings
            .into_iter()
            .next()
            .ok_or_else(|| ToolError::ExecutionFailed("empty embedding result".into()))?;

        let query = StoreQuery {
            dense_vector,
            query_text: query_text.to_owned(),
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

    /// Searches past conversation messages via full-text search.
    async fn search_conversations(
        &self,
        user_id: UserId,
        query: &str,
        input: &serde_json::Value,
        limit: i64,
    ) -> Result<ToolOutput, ToolError> {
        let conversation_id = input
            .get("conversation_id")
            .and_then(|v| v.as_str())
            .map(|s| s.parse::<uuid::Uuid>())
            .transpose()
            .map_err(|_| ToolError::InvalidInput("invalid conversation_id".into()))?
            .map(ConversationId::from_uuid);

        let hits = self
            .messages
            .search_by_user(user_id, query, conversation_id, limit)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("conversation search failed: {e}")))?;

        if hits.is_empty() {
            return Ok(ToolOutput {
                content: "No matching conversation messages found.".into(),
                is_error: false,
            });
        }

        let mut output = format!("Found {} matching message(s):\n\n", hits.len());
        for hit in &hits {
            output.push_str(&format!(
                "**[{}] {} — {}** (score: {:.2})\n{}\n\n",
                hit.created_at.format("%Y-%m-%d %H:%M"),
                hit.conversation_title.as_deref().unwrap_or("Untitled"),
                hit.role,
                hit.score,
                hit.content
            ));
        }
        Ok(ToolOutput {
            content: output,
            is_error: false,
        })
    }
}

impl<M: MessageRepo + Send + Sync + 'static> Tool for RecallTool<M> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "recall".to_owned(),
            description: "Search your memory or past conversations. Relevant memories are \
                already auto-loaded into your context each turn — use this tool for targeted \
                searches when you need something specific beyond what was loaded.\n\n\
                source: \"memory\" (default) — Search stored knowledge: personal facts, \
                preferences, learned skills, code snippets, decisions. Use when looking for \
                something specific you stored about this user.\n\n\
                source: \"conversations\" — Full-text search over past conversation messages. \
                Use for anything discussed previously: decisions, questions, technical context, \
                anything that was said but may not have been extracted into memory.\n\n\
                When to use:\n\
                - The user references a past conversation or decision\n\
                - You need specific context not present in auto-loaded memories\n\
                - Before saying \"I don't know\" — search both sources first\n\
                - When the user asks \"do you remember\" or \"we discussed\"\n\n\
                Do NOT call this at the start of every conversation — context is auto-loaded."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query, crafted for semantic relevance to what you're looking for."
                    },
                    "source": {
                        "type": "string",
                        "enum": ["memory", "conversations"],
                        "description": "Where to search. 'memory' (default) for stored knowledge — facts, preferences, skills, code. 'conversations' for past conversation messages — decisions, discussions, anything that was said."
                    },
                    "chunk_type": {
                        "type": "string",
                        "enum": ["fact", "conversation", "preference", "skill", "code", "soul"],
                        "description": "Filter results to a specific memory type. Only applies when source is 'memory'."
                    },
                    "scope": {
                        "type": "string",
                        "enum": ["user", "system"],
                        "description": "Search scope: 'user' for personal memories (default), 'system' for global knowledge. Only applies when source is 'memory'."
                    },
                    "conversation_id": {
                        "type": "string",
                        "description": "Narrow search to a specific conversation. Only applies when source is 'conversations'."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 10, max: 20)."
                    }
                },
                "required": ["query"]
            }),
            context_modifying: false,
            redacted: false,
            visibility: ToolVisibility::Public,
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
        let scope_id = resolve_scope(&input, user_id);

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
            description: "Store important information in long-term memory. Most extraction \
                happens automatically via extraction blocks, but use this tool directly when:\n\
                - The user explicitly asks you to remember something\n\
                - You need to store something complex that benefits from precise wording\n\
                - You want to store with a specific importance score or chunk type\n\
                - You realize mid-conversation that an earlier fact should be stored\n\n\
                Scope: 'user' (default) for personal details about the user. 'system' for \
                knowledge about yourself — capabilities, configuration, learned behaviors."
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
                    },
                    "scope": {
                        "type": "string",
                        "enum": ["user", "system"],
                        "description": "Storage scope: 'user' for personal memories (default), 'system' for knowledge about the agent itself (identity, capabilities, learned behaviors)."
                    }
                },
                "required": ["content", "chunk_type"]
            }),
            context_modifying: false,
            redacted: false,
            visibility: ToolVisibility::Public,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::pin::Pin;

    use async_trait::async_trait;
    use futures::Stream;
    use sober_core::config::QdrantConfig;
    use sober_core::error::AppError;
    use sober_core::types::domain::{Message, MessageSearchHit};
    use sober_core::types::ids::{ConversationId, MessageId};
    use sober_core::types::input::CreateMessage;
    use sober_llm::error::LlmError;
    use sober_llm::types::EngineCapabilities;
    use sober_llm::types::{CompletionRequest, CompletionResponse, StreamChunk};

    // -----------------------------------------------------------------------
    // Stub MessageRepo — methods are never called during validation tests
    // -----------------------------------------------------------------------

    struct StubMessageRepo;

    impl MessageRepo for StubMessageRepo {
        fn create(
            &self,
            _input: CreateMessage,
        ) -> impl std::future::Future<Output = Result<Message, AppError>> + Send {
            async { unimplemented!("stub") }
        }

        fn list_by_conversation(
            &self,
            _conversation_id: ConversationId,
            _limit: i64,
        ) -> impl std::future::Future<Output = Result<Vec<Message>, AppError>> + Send {
            async { unimplemented!("stub") }
        }

        fn list_paginated(
            &self,
            _conversation_id: ConversationId,
            _before: Option<MessageId>,
            _limit: i64,
        ) -> impl std::future::Future<Output = Result<Vec<Message>, AppError>> + Send {
            async { unimplemented!("stub") }
        }

        fn delete(
            &self,
            _id: MessageId,
        ) -> impl std::future::Future<Output = Result<(), AppError>> + Send {
            async { unimplemented!("stub") }
        }

        fn clear_conversation(
            &self,
            _conversation_id: ConversationId,
        ) -> impl std::future::Future<Output = Result<(), AppError>> + Send {
            async { unimplemented!("stub") }
        }

        fn get_by_id(
            &self,
            _id: MessageId,
        ) -> impl std::future::Future<Output = Result<Message, AppError>> + Send {
            async { unimplemented!("stub") }
        }

        fn update_content(
            &self,
            _id: MessageId,
            _content: &str,
            _reasoning: Option<&str>,
        ) -> impl std::future::Future<Output = Result<(), AppError>> + Send {
            async { unimplemented!("stub") }
        }

        fn search_by_user(
            &self,
            _user_id: UserId,
            _query: &str,
            _conversation_id: Option<ConversationId>,
            _limit: i64,
        ) -> impl std::future::Future<Output = Result<Vec<MessageSearchHit>, AppError>> + Send
        {
            async { unimplemented!("stub") }
        }
    }

    // -----------------------------------------------------------------------
    // Stub LlmEngine — methods are never called during validation tests
    // -----------------------------------------------------------------------

    struct StubLlm;

    #[async_trait]
    impl LlmEngine for StubLlm {
        async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
            unimplemented!("stub")
        }

        async fn stream(
            &self,
            _req: CompletionRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send>>, LlmError>
        {
            unimplemented!("stub")
        }

        async fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, LlmError> {
            unimplemented!("stub")
        }

        fn capabilities(&self) -> EngineCapabilities {
            EngineCapabilities {
                supports_tools: false,
                supports_streaming: false,
                supports_embeddings: false,
                max_context_tokens: 0,
            }
        }

        fn model_id(&self) -> &str {
            "stub/model"
        }
    }

    // -----------------------------------------------------------------------
    // Helper to build a RecallTool that satisfies the type system
    // -----------------------------------------------------------------------

    fn make_recall_tool() -> RecallTool<StubMessageRepo> {
        let config = QdrantConfig {
            url: "http://localhost:6334".to_owned(),
            api_key: None,
        };
        let store = MemoryStore::new(&config, 384).expect("qdrant client should build");
        RecallTool::new(
            Arc::new(store),
            Arc::new(StubLlm),
            MemoryConfig::default(),
            Arc::new(StubMessageRepo),
        )
    }

    // -----------------------------------------------------------------------
    // Validation tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn empty_query_rejected() {
        let tool = make_recall_tool();
        let input = serde_json::json!({
            "query": "",
            "owner_id": "00000000-0000-0000-0000-000000000001"
        });

        let err = tool
            .execute_inner(input)
            .await
            .expect_err("should reject empty query");
        let msg = err.to_string();
        assert!(
            msg.contains("must not be empty"),
            "expected 'must not be empty', got: {msg}"
        );
    }

    #[tokio::test]
    async fn oversized_query_rejected() {
        let tool = make_recall_tool();
        let long_query = "x".repeat(257);
        let input = serde_json::json!({
            "query": long_query,
            "owner_id": "00000000-0000-0000-0000-000000000001"
        });

        let err = tool
            .execute_inner(input)
            .await
            .expect_err("should reject oversized query");
        let msg = err.to_string();
        assert!(msg.contains("too long"), "expected 'too long', got: {msg}");
    }

    #[tokio::test]
    async fn unknown_source_rejected() {
        let tool = make_recall_tool();
        let input = serde_json::json!({
            "query": "test",
            "source": "invalid",
            "owner_id": "00000000-0000-0000-0000-000000000001"
        });

        let err = tool
            .execute_inner(input)
            .await
            .expect_err("should reject unknown source");
        let msg = err.to_string();
        assert!(
            msg.contains("unknown source"),
            "expected 'unknown source', got: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // Existing tests
    // -----------------------------------------------------------------------

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
