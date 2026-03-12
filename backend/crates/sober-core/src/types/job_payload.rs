//! Typed job payload — determines execution path in the agent.
//!
//! Serialized with bincode into the `payload_bytes` column.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Discriminated job payload — determines execution path in agent.
/// Serialized with bincode into the `payload_bytes` column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobPayload {
    /// Natural language prompt executed via LLM with workspace context.
    Prompt {
        /// The prompt text to execute.
        text: String,
        /// Workspace to load context from (None for system-level).
        workspace_id: Option<Uuid>,
        /// Optional model preference hint.
        model_hint: Option<String>,
    },
    /// Compiled artifact (WASM or script) executed in sandbox.
    Artifact {
        /// Content-addressed blob reference.
        blob_ref: String,
        /// Type of artifact to execute.
        artifact_type: ArtifactType,
        /// Workspace the artifact belongs to.
        workspace_id: Uuid,
        /// Environment variables for execution.
        env: HashMap<String, String>,
    },
    /// Internal operation dispatched directly to a crate method.
    /// No LLM involved — deterministic execution.
    Internal {
        /// Which internal operation to run.
        operation: InternalOp,
    },
}

impl JobPayload {
    /// Serialize to bytes for storage in `payload_bytes` column.
    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize from bytes stored in `payload_bytes` column.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }
}

/// Type of artifact to execute in sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArtifactType {
    /// WebAssembly module.
    Wasm,
    /// Executable script.
    Script,
}

/// Deterministic internal operations that don't need LLM mediation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InternalOp {
    /// Prune expired memory chunks, decay importance scores.
    MemoryPruning,
    /// Clean up expired sessions and temporary data.
    SessionCleanup,
    /// Optimize Qdrant indices, rebalance collections.
    VectorIndexOptimize,
    /// Audit installed plugins for security issues and updates.
    PluginAudit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_payload_roundtrip() {
        let payload = JobPayload::Prompt {
            text: "Check deploy status".into(),
            workspace_id: Some(Uuid::now_v7()),
            model_hint: None,
        };
        let bytes = payload.to_bytes().unwrap();
        let decoded = JobPayload::from_bytes(&bytes).unwrap();
        match decoded {
            JobPayload::Prompt { text, .. } => assert_eq!(text, "Check deploy status"),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn artifact_payload_roundtrip() {
        let payload = JobPayload::Artifact {
            blob_ref: "sha256:abc123".into(),
            artifact_type: ArtifactType::Wasm,
            workspace_id: Uuid::now_v7(),
            env: HashMap::from([("KEY".into(), "value".into())]),
        };
        let bytes = payload.to_bytes().unwrap();
        let decoded = JobPayload::from_bytes(&bytes).unwrap();
        match decoded {
            JobPayload::Artifact { blob_ref, .. } => assert_eq!(blob_ref, "sha256:abc123"),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn internal_payload_roundtrip() {
        let payload = JobPayload::Internal {
            operation: InternalOp::MemoryPruning,
        };
        let bytes = payload.to_bytes().unwrap();
        let decoded = JobPayload::from_bytes(&bytes).unwrap();
        assert!(matches!(
            decoded,
            JobPayload::Internal {
                operation: InternalOp::MemoryPruning
            }
        ));
    }
}
