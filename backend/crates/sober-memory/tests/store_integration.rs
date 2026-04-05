//! Integration tests for MemoryStore (require running Qdrant).
//!
//! Run with: `docker compose up -d && cargo test -p sober-memory -q`

use chrono::Utc;
use sober_core::config::{MemoryConfig, QdrantConfig};
use sober_core::{ConversationId, MessageId, ScopeId, UserId};
use sober_memory::store::{MemoryStore, conversation_collection_name};
use sober_memory::{ChunkType, CollectionTarget, StoreChunk, StoreOutcome, StoreQuery};

fn qdrant_config() -> QdrantConfig {
    QdrantConfig {
        url: std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_owned()),
        api_key: None,
    }
}

fn memory_config() -> MemoryConfig {
    MemoryConfig {
        decay_half_life_days: 30,
        retrieval_boost: 0.2,
        prune_threshold: 0.1,
        dedup_similarity_threshold: 0.92,
    }
}

fn test_chunk_with_vector(scope_id: ScopeId, content: &str, vector: Vec<f32>) -> StoreChunk {
    StoreChunk {
        dense_vector: vector,
        content: content.to_owned(),
        chunk_type: ChunkType::Fact,
        scope_id,
        source_message_id: Some(MessageId::new()),
        importance: 0.5,
        decay_at: Utc::now(),
    }
}

fn test_chunk(scope_id: ScopeId, content: &str) -> StoreChunk {
    StoreChunk {
        dense_vector: vec![0.1; 128],
        content: content.to_owned(),
        chunk_type: ChunkType::Fact,
        scope_id,
        source_message_id: Some(MessageId::new()),
        importance: 1.0,
        decay_at: Utc::now(),
    }
}

#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn ensure_collection_is_idempotent() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let user_id = UserId::new();

    store.ensure_collection(user_id).await.unwrap();
    store.ensure_collection(user_id).await.unwrap();
}

#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn store_and_search_roundtrip() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let user_id = UserId::new();
    let scope_id = ScopeId::from_uuid(*user_id.as_uuid());

    let chunk = test_chunk(scope_id, "Rust is a systems programming language");
    let point_id = store.store(user_id, chunk).await.unwrap();

    // Search with matching query
    let query = StoreQuery {
        dense_vector: vec![0.1; 128],
        query_text: "rust programming language".to_owned(),
        scope_id,
        limit: 10,
        score_threshold: None,
        chunk_type_filter: None,
    };
    let results = store.search(user_id, query).await.unwrap();

    assert!(!results.is_empty(), "expected at least one search result");

    let hit = results
        .iter()
        .find(|h| h.point_id == point_id)
        .expect("stored point should appear in results");
    assert_eq!(hit.content, "Rust is a systems programming language");
    assert_eq!(hit.chunk_type, ChunkType::Fact);
}

#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn delete_removes_point() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let user_id = UserId::new();
    let scope_id = ScopeId::from_uuid(*user_id.as_uuid());

    let chunk = test_chunk(scope_id, "temporary fact to delete");
    let point_id = store.store(user_id, chunk).await.unwrap();

    store.delete(user_id, scope_id, point_id).await.unwrap();

    let query = StoreQuery {
        dense_vector: vec![0.1; 128],
        query_text: "temporary fact".to_owned(),
        scope_id,
        limit: 10,
        score_threshold: None,
        chunk_type_filter: None,
    };
    let results = store.search(user_id, query).await.unwrap();
    assert!(
        !results.iter().any(|h| h.point_id == point_id),
        "deleted point should not appear in results"
    );
}

#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn prune_removes_expired_memories() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let user_id = UserId::new();
    let scope_id = ScopeId::from_uuid(*user_id.as_uuid());
    let config = memory_config();

    // Store a chunk with very old decay_at and low importance
    let chunk = StoreChunk {
        dense_vector: vec![0.1; 128],
        content: "old expired memory".to_owned(),
        chunk_type: ChunkType::Fact,
        scope_id,
        source_message_id: None,
        importance: 0.05, // below threshold after decay
        decay_at: Utc::now() - chrono::Duration::days(365),
    };
    let _point_id = store.store(user_id, chunk).await.unwrap();

    let pruned = store.prune(user_id, &config).await.unwrap();
    assert!(pruned > 0, "expected at least one point to be pruned");
}

#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn retrieval_boost_increases_importance() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let user_id = UserId::new();
    let scope_id = ScopeId::from_uuid(*user_id.as_uuid());
    let config = memory_config();

    let chunk = StoreChunk {
        dense_vector: vec![0.1; 128],
        content: "fact to boost".to_owned(),
        chunk_type: ChunkType::Fact,
        scope_id,
        source_message_id: None,
        importance: 0.5,
        decay_at: Utc::now(),
    };
    let point_id = store.store(user_id, chunk).await.unwrap();

    store
        .apply_retrieval_boost(user_id, scope_id, point_id, &config)
        .await
        .unwrap();

    // Search and verify the importance is boosted
    let query = StoreQuery {
        dense_vector: vec![0.1; 128],
        query_text: "fact boost".to_owned(),
        scope_id,
        limit: 10,
        score_threshold: None,
        chunk_type_filter: None,
    };
    let results = store.search(user_id, query).await.unwrap();
    let hit = results.iter().find(|h| h.point_id == point_id).unwrap();
    assert!(
        hit.importance > 0.5,
        "importance should be boosted from 0.5 to ~0.7, got {}",
        hit.importance
    );
}

#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn store_with_dedup_allows_unique_memories() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let config = memory_config();
    let user_id = UserId::new();
    let scope_id = ScopeId::from_uuid(*user_id.as_uuid());

    // Two orthogonal vectors — cosine similarity = 0
    let chunk_a = test_chunk_with_vector(scope_id, "fact about cats", vec![1.0; 128]);
    let chunk_b = test_chunk_with_vector(scope_id, "fact about dogs", vec![-1.0; 128]);

    let outcome_a = store
        .store_with_dedup(user_id, chunk_a, &config)
        .await
        .unwrap();
    let outcome_b = store
        .store_with_dedup(user_id, chunk_b, &config)
        .await
        .unwrap();

    assert!(
        matches!(outcome_a, StoreOutcome::Stored { .. }),
        "first unique chunk should be stored"
    );
    assert!(
        matches!(outcome_b, StoreOutcome::Stored { .. }),
        "second orthogonal chunk should also be stored"
    );
}

#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn store_with_dedup_detects_duplicate() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let config = memory_config();
    let user_id = UserId::new();
    let scope_id = ScopeId::from_uuid(*user_id.as_uuid());

    // Two identical vectors — cosine similarity = 1.0, well above threshold
    let chunk_a = test_chunk_with_vector(scope_id, "exact same fact", vec![0.5; 128]);
    let chunk_b = test_chunk_with_vector(scope_id, "exact same fact again", vec![0.5; 128]);

    let outcome_a = store
        .store_with_dedup(user_id, chunk_a, &config)
        .await
        .unwrap();
    assert!(
        matches!(outcome_a, StoreOutcome::Stored { .. }),
        "first chunk should be stored"
    );

    let outcome_b = store
        .store_with_dedup(user_id, chunk_b, &config)
        .await
        .unwrap();
    assert!(
        matches!(outcome_b, StoreOutcome::Deduplicated { .. }),
        "identical vector should be deduplicated"
    );
}

#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn store_with_dedup_respects_scope_isolation() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let config = memory_config();
    let user_id = UserId::new();

    let user_scope = ScopeId::from_uuid(*user_id.as_uuid());
    let conv_id = ConversationId::new();
    let conv_scope = ScopeId::from_uuid(*conv_id.as_uuid());

    // Same vector, different scopes — should both be stored
    let chunk_user = test_chunk_with_vector(user_scope, "scoped fact user", vec![0.5; 128]);
    let chunk_conv = test_chunk_with_vector(conv_scope, "scoped fact conv", vec![0.5; 128]);

    let outcome_user = store
        .store_with_dedup(user_id, chunk_user, &config)
        .await
        .unwrap();
    let outcome_conv = store
        .store_with_dedup(user_id, chunk_conv, &config)
        .await
        .unwrap();

    assert!(
        matches!(outcome_user, StoreOutcome::Stored { .. }),
        "user-scoped chunk should be stored"
    );
    assert!(
        matches!(outcome_conv, StoreOutcome::Stored { .. }),
        "conversation-scoped chunk in different scope should also be stored"
    );
}

#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn find_similar_returns_none_for_empty_collection() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let config = memory_config();
    let user_id = UserId::new();
    let scope_id = ScopeId::from_uuid(*user_id.as_uuid());

    // No collection created, no data stored
    let result = store
        .find_similar(
            user_id,
            scope_id,
            &vec![0.5; 128],
            config.dedup_similarity_threshold as f32,
        )
        .await
        .unwrap();

    assert!(result.is_none(), "empty collection should return None");
}

#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn store_routes_conversation_scope_to_conv_collection() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let config = memory_config();
    let user_id = UserId::new();
    let user_scope = ScopeId::from_uuid(*user_id.as_uuid());

    let conv_id = ConversationId::new();
    let conv_scope = ScopeId::from_uuid(*conv_id.as_uuid());

    let chunk = test_chunk_with_vector(conv_scope, "conversation-scoped fact", vec![0.3; 128]);
    let outcome = store
        .store_with_dedup(user_id, chunk, &config)
        .await
        .unwrap();
    assert!(
        matches!(outcome, StoreOutcome::Stored { .. }),
        "chunk should be stored"
    );

    // Search in conversation scope should find it
    let conv_query = StoreQuery {
        dense_vector: vec![0.3; 128],
        query_text: "conversation fact".to_owned(),
        scope_id: conv_scope,
        limit: 10,
        score_threshold: None,
        chunk_type_filter: None,
    };
    let conv_results = store.search(user_id, conv_query).await.unwrap();
    assert!(
        !conv_results.is_empty(),
        "should find chunk in conversation scope"
    );

    // Search in user scope should NOT find the conversation-scoped chunk
    let user_query = StoreQuery {
        dense_vector: vec![0.3; 128],
        query_text: "conversation fact".to_owned(),
        scope_id: user_scope,
        limit: 10,
        score_threshold: None,
        chunk_type_filter: None,
    };
    let user_results = store.search(user_id, user_query).await.unwrap();
    let found_in_user_scope = user_results.iter().any(|h| h.scope_id == conv_scope);
    assert!(
        !found_in_user_scope,
        "conversation-scoped chunk should not appear in user scope search"
    );
}

#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn deduplicate_merges_similar_points() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let config = memory_config();
    let user_id = UserId::new();
    let scope_id = ScopeId::from_uuid(*user_id.as_uuid());

    // Store two identical-vector points directly (bypassing write-time dedup)
    let chunk_a = test_chunk_with_vector(scope_id, "duplicate fact A", vec![0.7; 128]);
    let chunk_b = test_chunk_with_vector(scope_id, "duplicate fact B", vec![0.7; 128]);
    store.store(user_id, chunk_a).await.unwrap();
    store.store(user_id, chunk_b).await.unwrap();

    // Batch deduplication should merge them
    let stats = store
        .deduplicate(CollectionTarget::User(user_id), &config)
        .await
        .unwrap();

    assert!(
        stats.merged > 0,
        "batch dedup should have merged at least one duplicate, got stats: {:?}",
        stats
    );
}

#[tokio::test]
#[ignore = "requires running Qdrant"]
async fn delete_collection_removes_conversation_memories() {
    let store = MemoryStore::new(&qdrant_config(), 128).unwrap();
    let config = memory_config();
    let user_id = UserId::new();

    let conv_id = ConversationId::new();
    let conv_scope = ScopeId::from_uuid(*conv_id.as_uuid());

    // Store a chunk in conversation scope
    let chunk = test_chunk_with_vector(conv_scope, "ephemeral conversation fact", vec![0.4; 128]);
    store
        .store_with_dedup(user_id, chunk, &config)
        .await
        .unwrap();

    // Verify it exists
    let query = StoreQuery {
        dense_vector: vec![0.4; 128],
        query_text: "ephemeral fact".to_owned(),
        scope_id: conv_scope,
        limit: 10,
        score_threshold: None,
        chunk_type_filter: None,
    };
    let before = store.search(user_id, query.clone()).await.unwrap();
    assert!(!before.is_empty(), "chunk should exist before deletion");

    // Delete the conversation collection
    let coll_name = conversation_collection_name(conv_scope);
    store.delete_collection(&coll_name).await.unwrap();

    // After deletion, search should return empty (collection gone)
    let after = store.search(user_id, query).await.unwrap();
    assert!(
        after.is_empty(),
        "no results should be returned after collection deletion"
    );
}
