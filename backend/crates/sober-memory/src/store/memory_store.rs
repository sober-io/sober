//! Qdrant-backed memory store for vector storage and hybrid search.

use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use metrics::{counter, histogram};
use qdrant_client::Payload;
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, DeletePointsBuilder, Distance, Filter, Fusion,
    GetPointsBuilder, Modifier, NamedVectors, PointStruct, PointsIdsList, PrefetchQueryBuilder,
    Query, QueryPointsBuilder, ScrollPointsBuilder, SetPayloadPointsBuilder,
    SparseVectorParamsBuilder, SparseVectorsConfigBuilder, UpsertPointsBuilder, Vector,
    VectorInput, VectorParamsBuilder, VectorsConfigBuilder,
};
use serde_json::json;
use sober_core::config::{MemoryConfig, QdrantConfig};
use sober_core::{ScopeId, UserId};

use super::bm25;
use super::collections::{
    conversation_collection_name, system_collection_name, user_collection_name,
};
use super::types::ChunkType;
use super::types::{MemoryHit, StoreChunk, StoreQuery};
use crate::error::MemoryError;
use crate::scoring;

/// Qdrant payload field names.
mod fields {
    pub const SCOPE_ID: &str = "scope_id";
    pub const CHUNK_TYPE: &str = "chunk_type";
    pub const CONTENT: &str = "content";
    pub const SOURCE_MESSAGE_ID: &str = "source_message_id";
    pub const IMPORTANCE: &str = "importance";
    pub const CREATED_AT: &str = "created_at";
    pub const DECAY_AT: &str = "decay_at";
}

/// Dense vector name in Qdrant named vectors.
const DENSE_VECTOR_NAME: &str = "dense";

/// Sparse BM25 vector name in Qdrant named vectors.
const SPARSE_VECTOR_NAME: &str = "bm25";

/// Qdrant-backed vector memory store.
///
/// Manages per-user collections and a system collection, providing
/// hybrid dense + sparse BM25 search.
pub struct MemoryStore {
    client: Arc<Qdrant>,
    dense_vector_size: u64,
}

impl MemoryStore {
    /// Creates a new memory store connected to Qdrant.
    pub fn new(config: &QdrantConfig, dense_vector_size: u64) -> Result<Self, MemoryError> {
        let mut builder = Qdrant::from_url(&config.url);
        if let Some(ref api_key) = config.api_key {
            builder = builder.api_key(api_key.clone());
        }
        let client = builder
            .build()
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        Ok(Self {
            client: Arc::new(client),
            dense_vector_size,
        })
    }

    /// Creates a user's collection if it does not exist. Idempotent.
    pub async fn ensure_collection(&self, user_id: UserId) -> Result<(), MemoryError> {
        let name = user_collection_name(user_id);
        self.create_collection_if_missing(&name).await
    }

    /// Creates the system collection if it does not exist. Idempotent.
    pub async fn ensure_system_collection(&self) -> Result<(), MemoryError> {
        self.create_collection_if_missing(system_collection_name())
            .await
    }

    /// Stores a memory chunk in the appropriate collection.
    ///
    /// Routes to the system collection when `scope_id` is [`ScopeId::GLOBAL`],
    /// otherwise to the user's collection.
    ///
    /// Returns the assigned point UUID.
    pub async fn store(
        &self,
        user_id: UserId,
        chunk: StoreChunk,
    ) -> Result<uuid::Uuid, MemoryError> {
        let chunk_type_label = chunk.chunk_type.to_string();
        let scope_label = if chunk.scope_id == sober_core::ScopeId::GLOBAL {
            "global"
        } else if chunk.scope_id == ScopeId::from_uuid(*user_id.as_uuid()) {
            "user"
        } else {
            "conversation"
        };

        let collection = self.collection_for_scope(user_id, chunk.scope_id);
        self.create_collection_if_missing(&collection).await?;

        let point_id = uuid::Uuid::now_v7();
        let sparse = bm25::compute_sparse_vector(&chunk.content);

        let sparse_indices: Vec<u32> = sparse.iter().map(|(i, _)| *i).collect();
        let sparse_values: Vec<f32> = sparse.iter().map(|(_, v)| *v).collect();
        let sparse_vector = Vector::new_sparse(sparse_indices, sparse_values);

        let mut named = NamedVectors::default();
        named = named.add_vector(DENSE_VECTOR_NAME, chunk.dense_vector);
        named = named.add_vector(SPARSE_VECTOR_NAME, sparse_vector);

        let payload = Payload::try_from(json!({
            fields::SCOPE_ID: chunk.scope_id.to_string(),
            fields::CHUNK_TYPE: u8::from(chunk.chunk_type),
            fields::CONTENT: chunk.content,
            fields::SOURCE_MESSAGE_ID: chunk.source_message_id.map(|id| id.to_string()),
            fields::IMPORTANCE: chunk.importance,
            fields::CREATED_AT: Utc::now().to_rfc3339(),
            fields::DECAY_AT: chunk.decay_at.to_rfc3339(),
        }))
        .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        let point = PointStruct::new(point_id.to_string(), named, payload);

        self.client
            .upsert_points(UpsertPointsBuilder::new(&collection, vec![point]).wait(true))
            .await
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        counter!("sober_memory_chunks_stored_total", "chunk_type" => chunk_type_label, "scope" => scope_label).increment(1);

        Ok(point_id)
    }

    /// Performs a hybrid dense + BM25 search over a user's collection.
    ///
    /// Returns an empty result set when `limit` is 0 or `dense_vector` is empty
    /// (e.g. when the embedding step was skipped).
    pub async fn search(
        &self,
        user_id: UserId,
        query: StoreQuery,
    ) -> Result<Vec<MemoryHit>, MemoryError> {
        if query.limit == 0 || query.dense_vector.is_empty() {
            return Ok(Vec::new());
        }

        let scope_label = if query.scope_id == sober_core::ScopeId::GLOBAL {
            "global"
        } else if query.scope_id == ScopeId::from_uuid(*user_id.as_uuid()) {
            "user"
        } else {
            "conversation"
        };
        let search_type = "hybrid";
        let start = Instant::now();

        let collection = self.collection_for_scope(user_id, query.scope_id);

        let sparse = bm25::compute_sparse_vector(&query.query_text);
        let sparse_indices: Vec<u32> = sparse.iter().map(|(i, _)| *i).collect();
        let sparse_values: Vec<f32> = sparse.iter().map(|(_, v)| *v).collect();

        let mut conditions: Vec<Condition> = vec![Condition::matches(
            fields::SCOPE_ID,
            query.scope_id.to_string(),
        )];
        if let Some(ct) = query.chunk_type_filter {
            conditions.push(Condition::matches(fields::CHUNK_TYPE, ct as i64));
        }
        let scope_filter = Filter::must(conditions);

        // Prefetch: dense search
        let dense_prefetch = PrefetchQueryBuilder::default()
            .query(VectorInput::new_dense(query.dense_vector))
            .using(DENSE_VECTOR_NAME)
            .filter(scope_filter.clone())
            .limit(query.limit * 2);

        // Prefetch: sparse BM25 search
        let sparse_prefetch = PrefetchQueryBuilder::default()
            .query(VectorInput::new_sparse(sparse_indices, sparse_values))
            .using(SPARSE_VECTOR_NAME)
            .filter(scope_filter)
            .limit(query.limit * 2);

        // Fuse with RRF
        let mut qb = QueryPointsBuilder::new(&collection)
            .query(Query::new_fusion(Fusion::Rrf))
            .add_prefetch(dense_prefetch)
            .add_prefetch(sparse_prefetch)
            .limit(query.limit)
            .with_payload(true);

        if let Some(threshold) = query.score_threshold {
            qb = qb.score_threshold(threshold);
        }

        let result = self
            .client
            .query(qb)
            .await
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        let mut hits = Vec::with_capacity(result.result.len());
        for point in result.result {
            if let Some(hit) = self.scored_point_to_hit(&point) {
                hits.push(hit);
            }
        }

        let elapsed = start.elapsed().as_secs_f64();
        counter!("sober_memory_search_total", "scope" => scope_label, "search_type" => search_type)
            .increment(1);
        histogram!("sober_memory_search_duration_seconds", "scope" => scope_label, "search_type" => search_type).record(elapsed);
        histogram!("sober_memory_search_results_count").record(hits.len() as f64);

        Ok(hits)
    }

    /// Deletes a point by UUID from the appropriate collection.
    pub async fn delete(
        &self,
        user_id: UserId,
        scope_id: ScopeId,
        point_id: uuid::Uuid,
    ) -> Result<(), MemoryError> {
        let collection = self.collection_for_scope(user_id, scope_id);

        self.client
            .delete_points(
                DeletePointsBuilder::new(&collection)
                    .points(PointsIdsList {
                        ids: vec![point_id.to_string().into()],
                    })
                    .wait(true),
            )
            .await
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        Ok(())
    }

    /// Prunes memories with decayed importance below the configured threshold.
    ///
    /// Scrolls through all points, computes current decayed importance,
    /// and deletes those below `config.prune_threshold`.
    ///
    /// Returns the count of pruned points.
    pub async fn prune(&self, user_id: UserId, config: &MemoryConfig) -> Result<u64, MemoryError> {
        let start = Instant::now();
        let collection = user_collection_name(user_id);
        let now = Utc::now();
        let mut pruned: u64 = 0;
        let mut offset: Option<qdrant_client::qdrant::PointId> = None;

        loop {
            let mut sb = ScrollPointsBuilder::new(&collection)
                .limit(100)
                .with_payload(true)
                .with_vectors(false);

            if let Some(ref o) = offset {
                sb = sb.offset(o.clone());
            }

            let result = self
                .client
                .scroll(sb)
                .await
                .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

            let mut to_delete = Vec::new();

            for point in &result.result {
                let importance =
                    Self::payload_f64(&point.payload, fields::IMPORTANCE).unwrap_or(1.0);
                let decay_at = Self::payload_str(&point.payload, fields::DECAY_AT)
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc));

                let elapsed_days = decay_at
                    .map(|dt| (now - dt).num_seconds().max(0) as f64 / 86400.0)
                    .unwrap_or(0.0);

                let current = scoring::decay(importance, elapsed_days, config.decay_half_life_days);

                if scoring::should_prune(current, config.prune_threshold) {
                    to_delete.push(point.id.clone());
                }
            }

            if !to_delete.is_empty() {
                let ids: Vec<_> = to_delete.into_iter().flatten().collect();
                pruned += ids.len() as u64;
                self.client
                    .delete_points(
                        DeletePointsBuilder::new(&collection)
                            .points(PointsIdsList { ids })
                            .wait(true),
                    )
                    .await
                    .map_err(|e| MemoryError::Qdrant(e.to_string()))?;
            }

            match result.next_page_offset {
                Some(next) => offset = Some(next),
                None => break,
            }
        }

        let elapsed = start.elapsed().as_secs_f64();
        counter!("sober_memory_prune_runs_total").increment(1);
        histogram!("sober_memory_prune_duration_seconds").record(elapsed);
        counter!("sober_memory_pruned_chunks_total").increment(pruned);

        Ok(pruned)
    }

    /// Boosts the importance score of a retrieved point.
    ///
    /// Reads the current importance, applies the configured boost (capped
    /// at 1.0), and writes it back.
    pub async fn apply_retrieval_boost(
        &self,
        user_id: UserId,
        scope_id: ScopeId,
        point_id: uuid::Uuid,
        config: &MemoryConfig,
    ) -> Result<(), MemoryError> {
        let collection = self.collection_for_scope(user_id, scope_id);
        let point_id_str = point_id.to_string();

        // Read current importance
        let get_result = self
            .client
            .get_points(
                GetPointsBuilder::new(&collection, vec![point_id_str.clone().into()])
                    .with_payload(true)
                    .with_vectors(false),
            )
            .await
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        let current = get_result
            .result
            .first()
            .and_then(|p| Self::payload_f64(&p.payload, fields::IMPORTANCE))
            .unwrap_or(1.0);

        let boosted = scoring::boost(current, config.retrieval_boost);

        // Write boosted importance back
        let payload = Payload::try_from(json!({
            fields::IMPORTANCE: boosted,
        }))
        .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        self.client
            .set_payload(
                SetPayloadPointsBuilder::new(&collection, payload)
                    .points_selector(PointsIdsList {
                        ids: vec![point_id_str.into()],
                    })
                    .wait(true),
            )
            .await
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        Ok(())
    }

    /// Searches for a memory that is semantically similar to the given dense vector.
    ///
    /// Performs a dense-only cosine query (no BM25/RRF) limited to the given scope.
    /// Returns `None` when the collection does not exist or no result meets the threshold.
    pub async fn find_similar(
        &self,
        user_id: UserId,
        scope_id: ScopeId,
        dense_vector: &[f32],
        threshold: f32,
    ) -> Result<Option<MemoryHit>, MemoryError> {
        let start = Instant::now();
        let collection = self.collection_for_scope(user_id, scope_id);

        // Short-circuit when the collection hasn't been created yet.
        let exists = self
            .client
            .collection_exists(&collection)
            .await
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        if !exists {
            histogram!("sober_memory_dedup_check_duration_seconds")
                .record(start.elapsed().as_secs_f64());
            return Ok(None);
        }

        let scope_filter = Filter::must(vec![Condition::matches(
            fields::SCOPE_ID,
            scope_id.to_string(),
        )]);

        let qb = QueryPointsBuilder::new(&collection)
            .query(VectorInput::new_dense(dense_vector.to_vec()))
            .using(DENSE_VECTOR_NAME)
            .filter(scope_filter)
            .score_threshold(threshold)
            .limit(1)
            .with_payload(true);

        let result = self
            .client
            .query(qb)
            .await
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        histogram!("sober_memory_dedup_check_duration_seconds")
            .record(start.elapsed().as_secs_f64());

        let hit = result
            .result
            .first()
            .and_then(|p| self.scored_point_to_hit(p));

        Ok(hit)
    }

    /// Stores a memory chunk with write-time deduplication.
    ///
    /// If a sufficiently similar memory already exists (cosine similarity >=
    /// `config.dedup_similarity_threshold`), its importance is boosted and
    /// [`StoreOutcome::Deduplicated`] is returned. Otherwise the chunk is stored
    /// normally and [`StoreOutcome::Stored`] is returned.
    ///
    /// Setting `dedup_similarity_threshold` to `1.0` in config disables dedup
    /// (a vector can never be 100% similar to a different vector in practice).
    pub async fn store_with_dedup(
        &self,
        user_id: UserId,
        chunk: StoreChunk,
        config: &MemoryConfig,
    ) -> Result<super::types::StoreOutcome, MemoryError> {
        let threshold = config.dedup_similarity_threshold as f32;
        let chunk_type_label = chunk.chunk_type.to_string();

        // Threshold >= 1.0 means dedup is disabled — skip the similarity check.
        if threshold >= 1.0 {
            let point_id = self.store(user_id, chunk).await?;
            counter!("sober_memory_dedup_total", "outcome" => "stored", "chunk_type" => chunk_type_label)
                .increment(1);
            return Ok(super::types::StoreOutcome::Stored { point_id });
        }

        let scope_id = chunk.scope_id;
        if let Some(existing) = self
            .find_similar(user_id, scope_id, &chunk.dense_vector, threshold)
            .await?
        {
            let existing_point_id = existing.point_id;
            let similarity = existing.score;
            self.apply_retrieval_boost(user_id, scope_id, existing_point_id, config)
                .await?;
            counter!("sober_memory_dedup_total", "outcome" => "deduplicated", "chunk_type" => chunk_type_label)
                .increment(1);
            return Ok(super::types::StoreOutcome::Deduplicated {
                existing_point_id,
                similarity,
            });
        }

        let point_id = self.store(user_id, chunk).await?;
        counter!("sober_memory_dedup_total", "outcome" => "stored", "chunk_type" => chunk_type_label)
            .increment(1);
        Ok(super::types::StoreOutcome::Stored { point_id })
    }

    /// Deletes a Qdrant collection by name.
    ///
    /// No-op if the collection does not exist.
    pub async fn delete_collection(&self, name: &str) -> Result<(), MemoryError> {
        let exists = self
            .client
            .collection_exists(name)
            .await
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        if !exists {
            return Ok(());
        }

        self.client
            .delete_collection(name)
            .await
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        tracing::info!(collection = name, "deleted qdrant collection");
        Ok(())
    }

    /// Returns the names of all existing Qdrant collections.
    pub async fn list_collections(&self) -> Result<Vec<String>, MemoryError> {
        let response = self
            .client
            .list_collections()
            .await
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        Ok(response.collections.into_iter().map(|c| c.name).collect())
    }

    /// Scans the target collection and removes near-duplicate points.
    ///
    /// For each scope group, computes pairwise cosine similarity between
    /// all points. When a pair exceeds `config.dedup_similarity_threshold`,
    /// the point with lower importance (tiebreak: newer `created_at`) is
    /// marked for deletion. Deletes are flushed in batches of 50.
    ///
    /// Returns [`DedupStats`] with the number of points scanned and merged.
    pub async fn deduplicate(
        &self,
        target: super::types::CollectionTarget,
        config: &MemoryConfig,
    ) -> Result<super::types::DedupStats, MemoryError> {
        use super::types::CollectionTarget;

        let start = Instant::now();

        let collection = match target {
            CollectionTarget::User(user_id) => user_collection_name(user_id),
            CollectionTarget::Conversation(conv_id) => {
                conversation_collection_name(ScopeId::from_uuid(*conv_id.as_uuid()))
            }
            CollectionTarget::System => system_collection_name().to_owned(),
        };

        let exists = self
            .client
            .collection_exists(&collection)
            .await
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

        if !exists {
            return Ok(super::types::DedupStats::default());
        }

        let threshold = config.dedup_similarity_threshold as f32;
        let mut stats = super::types::DedupStats::default();
        let mut to_delete: Vec<qdrant_client::qdrant::PointId> = Vec::new();
        let mut offset: Option<qdrant_client::qdrant::PointId> = None;

        loop {
            let mut sb = ScrollPointsBuilder::new(&collection)
                .limit(100)
                .with_payload(true)
                .with_vectors(true);

            if let Some(ref o) = offset {
                sb = sb.offset(o.clone());
            }

            let result = self
                .client
                .scroll(sb)
                .await
                .map_err(|e| MemoryError::Qdrant(e.to_string()))?;

            // Group points by scope_id within the batch.
            let mut scope_groups: std::collections::HashMap<
                String,
                Vec<qdrant_client::qdrant::RetrievedPoint>,
            > = std::collections::HashMap::new();

            for point in result.result {
                stats.scanned += 1;
                let scope_id =
                    Self::payload_str(&point.payload, fields::SCOPE_ID).unwrap_or_default();
                scope_groups.entry(scope_id).or_default().push(point);
            }

            // Within each scope group, find pairs above the similarity threshold.
            let mut marked: std::collections::HashSet<String> = std::collections::HashSet::new();

            for points in scope_groups.values() {
                for i in 0..points.len() {
                    for j in (i + 1)..points.len() {
                        let a = &points[i];
                        let b = &points[j];

                        let id_a =
                            a.id.as_ref()
                                .and_then(|id| {
                                    use qdrant_client::qdrant::point_id::PointIdOptions;
                                    match &id.point_id_options {
                                        Some(PointIdOptions::Uuid(s)) => Some(s.clone()),
                                        _ => None,
                                    }
                                })
                                .unwrap_or_default();

                        let id_b =
                            b.id.as_ref()
                                .and_then(|id| {
                                    use qdrant_client::qdrant::point_id::PointIdOptions;
                                    match &id.point_id_options {
                                        Some(PointIdOptions::Uuid(s)) => Some(s.clone()),
                                        _ => None,
                                    }
                                })
                                .unwrap_or_default();

                        if marked.contains(&id_a) || marked.contains(&id_b) {
                            continue;
                        }

                        let similarity = Self::cosine_similarity_from_points(a, b);
                        if similarity < threshold {
                            continue;
                        }

                        // Keep higher importance; tiebreak: keep older (smaller created_at).
                        let imp_a =
                            Self::payload_f64(&a.payload, fields::IMPORTANCE).unwrap_or(1.0);
                        let imp_b =
                            Self::payload_f64(&b.payload, fields::IMPORTANCE).unwrap_or(1.0);

                        let discard_id = if (imp_a - imp_b).abs() > f64::EPSILON {
                            if imp_a >= imp_b {
                                id_b.clone()
                            } else {
                                id_a.clone()
                            }
                        } else {
                            // Tiebreak: keep the older one (discard the newer).
                            let created_a = Self::payload_str(&a.payload, fields::CREATED_AT)
                                .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                                .map(|dt| dt.timestamp());
                            let created_b = Self::payload_str(&b.payload, fields::CREATED_AT)
                                .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                                .map(|dt| dt.timestamp());
                            match (created_a, created_b) {
                                (Some(ta), Some(tb)) => {
                                    if ta <= tb {
                                        id_b.clone()
                                    } else {
                                        id_a.clone()
                                    }
                                }
                                _ => id_b.clone(),
                            }
                        };

                        marked.insert(discard_id.clone());

                        if let Ok(uuid) = uuid::Uuid::parse_str(&discard_id) {
                            to_delete.push(uuid.to_string().into());
                        }

                        // Flush deletes in batches of 50.
                        if to_delete.len() >= 50 {
                            let batch = std::mem::take(&mut to_delete);
                            let batch_len = batch.len() as u64;
                            self.client
                                .delete_points(
                                    DeletePointsBuilder::new(&collection)
                                        .points(PointsIdsList { ids: batch })
                                        .wait(true),
                                )
                                .await
                                .map_err(|e| MemoryError::Qdrant(e.to_string()))?;
                            stats.merged += batch_len;
                        }
                    }
                }
            }

            match result.next_page_offset {
                Some(next) => offset = Some(next),
                None => break,
            }
        }

        // Flush any remaining deletes.
        if !to_delete.is_empty() {
            let remaining = to_delete.len() as u64;
            self.client
                .delete_points(
                    DeletePointsBuilder::new(&collection)
                        .points(PointsIdsList { ids: to_delete })
                        .wait(true),
                )
                .await
                .map_err(|e| MemoryError::Qdrant(e.to_string()))?;
            stats.merged += remaining;
        }

        let elapsed = start.elapsed().as_secs_f64();
        counter!("sober_memory_batch_dedup_runs_total").increment(1);
        histogram!("sober_memory_batch_dedup_duration_seconds").record(elapsed);
        counter!("sober_memory_batch_dedup_merged_total").increment(stats.merged);

        Ok(stats)
    }

    // -- Private helpers --

    /// Determines the collection name based on scope.
    fn collection_for_scope(&self, user_id: UserId, scope_id: ScopeId) -> String {
        if scope_id == ScopeId::GLOBAL {
            system_collection_name().to_owned()
        } else if scope_id == ScopeId::from_uuid(*user_id.as_uuid()) {
            user_collection_name(user_id)
        } else {
            conversation_collection_name(scope_id)
        }
    }

    /// Creates a collection with dense + sparse vector config if it doesn't exist.
    async fn create_collection_if_missing(&self, name: &str) -> Result<(), MemoryError> {
        // Check if collection exists first
        if self
            .client
            .collection_exists(name)
            .await
            .map_err(|e| MemoryError::Qdrant(e.to_string()))?
        {
            return Ok(());
        }

        let mut dense_config = VectorsConfigBuilder::default();
        dense_config.add_named_vector_params(
            DENSE_VECTOR_NAME,
            VectorParamsBuilder::new(self.dense_vector_size, Distance::Cosine),
        );

        let mut sparse_config = SparseVectorsConfigBuilder::default();
        sparse_config.add_named_vector_params(
            SPARSE_VECTOR_NAME,
            SparseVectorParamsBuilder::default().modifier(Modifier::Idf),
        );

        let create_result = self
            .client
            .create_collection(
                CreateCollectionBuilder::new(name)
                    .vectors_config(dense_config)
                    .sparse_vectors_config(sparse_config),
            )
            .await;

        match create_result {
            Ok(_) => {
                tracing::info!(collection = name, "created qdrant collection");
            }
            Err(e) if e.to_string().contains("already exists") => {
                tracing::debug!(
                    collection = name,
                    "collection already exists (concurrent create)"
                );
            }
            Err(e) => return Err(MemoryError::Qdrant(e.to_string())),
        }

        Ok(())
    }

    /// Extracts a string from a Qdrant payload map.
    fn payload_str(
        payload: &std::collections::HashMap<String, qdrant_client::qdrant::Value>,
        key: &str,
    ) -> Option<String> {
        payload
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned())
    }

    /// Extracts an f64 from a Qdrant payload map.
    fn payload_f64(
        payload: &std::collections::HashMap<String, qdrant_client::qdrant::Value>,
        key: &str,
    ) -> Option<f64> {
        payload.get(key).and_then(|v| v.as_double())
    }

    /// Extracts a u64 from a Qdrant payload map (stored as integer).
    fn payload_u64(
        payload: &std::collections::HashMap<String, qdrant_client::qdrant::Value>,
        key: &str,
    ) -> Option<u64> {
        payload
            .get(key)
            .and_then(|v| v.as_integer())
            .map(|i| i as u64)
    }

    /// Computes the cosine similarity between the dense vectors of two [`RetrievedPoint`]s.
    ///
    /// Returns `0.0` if either point has no dense vector or if the vectors have
    /// mismatched lengths.
    fn cosine_similarity_from_points(
        a: &qdrant_client::qdrant::RetrievedPoint,
        b: &qdrant_client::qdrant::RetrievedPoint,
    ) -> f32 {
        fn extract_dense(point: &qdrant_client::qdrant::RetrievedPoint) -> Option<Vec<f32>> {
            use qdrant_client::qdrant::vector_output::Vector;
            use qdrant_client::qdrant::vectors_output::VectorsOptions;

            let vectors = point.vectors.as_ref()?;
            match vectors.vectors_options.as_ref()? {
                VectorsOptions::Vectors(named) => {
                    let vec_output = named.vectors.get(DENSE_VECTOR_NAME)?;
                    match vec_output.vector.as_ref()? {
                        Vector::Dense(dv) => Some(dv.data.clone()),
                        _ => None,
                    }
                }
                _ => None,
            }
        }

        let va = match extract_dense(a) {
            Some(v) => v,
            None => return 0.0,
        };
        let vb = match extract_dense(b) {
            Some(v) => v,
            None => return 0.0,
        };

        if va.len() != vb.len() || va.is_empty() {
            return 0.0;
        }

        let dot: f32 = va.iter().zip(vb.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = va.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = vb.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot / (norm_a * norm_b)
    }

    /// Converts a scored point from Qdrant into a [`MemoryHit`].
    fn scored_point_to_hit(&self, point: &qdrant_client::qdrant::ScoredPoint) -> Option<MemoryHit> {
        let point_id = point.id.as_ref().and_then(|id| {
            use qdrant_client::qdrant::point_id::PointIdOptions;
            match &id.point_id_options {
                Some(PointIdOptions::Uuid(s)) => uuid::Uuid::parse_str(s).ok(),
                Some(PointIdOptions::Num(n)) => {
                    // Fallback: shouldn't happen as we use UUIDs
                    tracing::warn!(num_id = n, "unexpected numeric point ID");
                    None
                }
                None => None,
            }
        })?;

        let payload = &point.payload;

        let content = Self::payload_str(payload, fields::CONTENT)?;
        let chunk_type_raw = Self::payload_u64(payload, fields::CHUNK_TYPE)? as u8;
        let chunk_type = ChunkType::try_from(chunk_type_raw).ok()?;
        let scope_id_str = Self::payload_str(payload, fields::SCOPE_ID)?;
        let scope_id = uuid::Uuid::parse_str(&scope_id_str)
            .ok()
            .map(ScopeId::from_uuid)?;

        let source_message_id = Self::payload_str(payload, fields::SOURCE_MESSAGE_ID)
            .and_then(|s| uuid::Uuid::parse_str(&s).ok())
            .map(sober_core::MessageId::from_uuid);

        let importance = Self::payload_f64(payload, fields::IMPORTANCE).unwrap_or(1.0);

        let created_at = Self::payload_str(payload, fields::CREATED_AT)
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        let decay_at = Self::payload_str(payload, fields::DECAY_AT)
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or(created_at);

        Some(MemoryHit {
            point_id,
            content,
            chunk_type,
            scope_id,
            source_message_id,
            importance,
            score: point.score,
            created_at,
            decay_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sober_core::ConversationId;

    fn test_store() -> MemoryStore {
        MemoryStore {
            client: Arc::new(Qdrant::from_url("http://localhost:6334").build().unwrap()),
            dense_vector_size: 128,
        }
    }

    #[test]
    fn collection_for_scope_routes_global_to_system() {
        let store = test_store();
        let user_id = UserId::new();
        let name = store.collection_for_scope(user_id, ScopeId::GLOBAL);
        assert_eq!(name, "system");
    }

    #[test]
    fn collection_for_scope_routes_user_to_user_collection() {
        let store = test_store();
        let user_id = UserId::new();
        let user_scope = ScopeId::from_uuid(*user_id.as_uuid());
        let name = store.collection_for_scope(user_id, user_scope);
        assert!(
            name.starts_with("user_"),
            "expected user_ prefix, got {name}"
        );
    }

    #[test]
    fn collection_for_scope_routes_conversation_to_conv_collection() {
        let store = test_store();
        let user_id = UserId::new();
        let conv_id = ConversationId::new();
        let conv_scope = ScopeId::from_uuid(*conv_id.as_uuid());
        let name = store.collection_for_scope(user_id, conv_scope);
        assert!(
            name.starts_with("conv_"),
            "expected conv_ prefix, got {name}"
        );
    }
}
