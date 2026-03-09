//! Soul layer management via sober-memory.
//!
//! Soul layers are per-user/group adaptations stored as BCF `Soul` chunks in
//! the vector store. They capture communication preferences, domain emphasis,
//! and interaction patterns learned over time.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sober_core::ScopeId;
use sober_memory::{ChunkType, MemoryStore, StoreChunk, StoreQuery};

use crate::error::MindError;

/// A soul layer — per-user or per-group personality adaptation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoulLayer {
    /// Which scope this layer applies to (user or group).
    pub scope_id: ScopeId,
    /// Key-value adaptations (e.g., `"tone" -> "formal"`).
    pub adaptations: Vec<SoulAdaptation>,
    /// Confidence score for this layer (0.0..=1.0).
    pub confidence: f64,
    /// When this layer was last updated.
    pub updated_at: DateTime<Utc>,
}

/// A single adaptation within a soul layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoulAdaptation {
    /// Adaptation key (e.g., "tone", "verbosity", "domain_focus").
    pub key: String,
    /// Adaptation value.
    pub value: String,
}

/// Loads soul layers from memory for a given scope.
///
/// Queries the memory store for `Soul` chunk types within the specified scope
/// and deserializes them into [`SoulLayer`] instances.
pub async fn load_soul_layers(
    memory: &MemoryStore,
    user_id: sober_core::UserId,
    scope_id: ScopeId,
    query_vector: Vec<f32>,
) -> Result<Vec<SoulLayer>, MindError> {
    let query = StoreQuery {
        dense_vector: query_vector,
        query_text: "soul layer adaptation".to_owned(),
        scope_id,
        limit: 10,
        score_threshold: None,
    };

    let hits = memory
        .search(user_id, query)
        .await
        .map_err(|e| MindError::LayerStoreFailed(e.to_string()))?;

    let mut layers = Vec::new();
    for hit in hits {
        if hit.chunk_type == ChunkType::Soul {
            if let Ok(layer) = serde_json::from_str::<SoulLayer>(&hit.content) {
                layers.push(layer);
            } else {
                tracing::warn!(
                    point_id = %hit.point_id,
                    "failed to deserialize soul layer, skipping"
                );
            }
        }
    }

    Ok(layers)
}

/// Stores a soul layer in memory for a given scope.
pub async fn store_soul_layer(
    memory: &MemoryStore,
    user_id: sober_core::UserId,
    layer: &SoulLayer,
    embedding: Vec<f32>,
) -> Result<uuid::Uuid, MindError> {
    let content =
        serde_json::to_string(layer).map_err(|e| MindError::LayerStoreFailed(e.to_string()))?;

    let chunk = StoreChunk {
        dense_vector: embedding,
        content,
        chunk_type: ChunkType::Soul,
        scope_id: layer.scope_id,
        source_message_id: None,
        importance: layer.confidence,
        decay_at: Utc::now() + chrono::Duration::days(365),
    };

    memory
        .store(user_id, chunk)
        .await
        .map_err(|e| MindError::LayerStoreFailed(e.to_string()))
}

/// Renders soul layers into a text block for inclusion in the prompt.
#[must_use]
pub fn render_layers(layers: &[SoulLayer]) -> String {
    if layers.is_empty() {
        return String::new();
    }

    let mut output = String::from("## Learned Adaptations\n\n");
    for layer in layers {
        for adaptation in &layer.adaptations {
            output.push_str(&format!(
                "- **{}**: {} (confidence: {:.0}%)\n",
                adaptation.key,
                adaptation.value,
                layer.confidence * 100.0
            ));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soul_layer_serialization_roundtrip() {
        let layer = SoulLayer {
            scope_id: ScopeId::new(),
            adaptations: vec![
                SoulAdaptation {
                    key: "tone".into(),
                    value: "formal".into(),
                },
                SoulAdaptation {
                    key: "verbosity".into(),
                    value: "concise".into(),
                },
            ],
            confidence: 0.85,
            updated_at: Utc::now(),
        };

        let json = serde_json::to_string(&layer).unwrap();
        let deserialized: SoulLayer = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.adaptations.len(), 2);
        assert_eq!(deserialized.adaptations[0].key, "tone");
        assert!((deserialized.confidence - 0.85).abs() < f64::EPSILON);
    }

    #[test]
    fn render_empty_layers() {
        assert_eq!(render_layers(&[]), "");
    }

    #[test]
    fn render_layers_with_content() {
        let layers = vec![SoulLayer {
            scope_id: ScopeId::new(),
            adaptations: vec![SoulAdaptation {
                key: "tone".into(),
                value: "casual".into(),
            }],
            confidence: 0.9,
            updated_at: Utc::now(),
        }];

        let rendered = render_layers(&layers);
        assert!(rendered.contains("Learned Adaptations"));
        assert!(rendered.contains("tone"));
        assert!(rendered.contains("casual"));
        assert!(rendered.contains("90%"));
    }
}
