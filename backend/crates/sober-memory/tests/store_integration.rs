//! Integration tests for MemoryStore (require running Qdrant).
//!
//! Run with: `docker compose up -d && cargo test -p sober-memory -q`

use chrono::Utc;
use sober_core::config::{MemoryConfig, QdrantConfig};
use sober_core::{MessageId, ScopeId, UserId};
use sober_memory::store::MemoryStore;
use sober_memory::{ChunkType, StoreChunk, StoreQuery};

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
