//! Trait evolution and confidence scoring (v1 stub).
//!
//! Trait evolution enables the agent to autonomously refine per-user/group
//! soul layers based on interaction patterns. In v1, all candidates are
//! queued for admin review — autonomous adoption is deferred to post-v1.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A candidate trait observation for potential adoption into a soul layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitCandidate {
    /// The observed pattern or preference.
    pub observation: String,
    /// Confidence score (0.0..=1.0) based on consistency, non-contradiction,
    /// and stability across interactions.
    pub confidence_score: f64,
    /// Number of distinct contexts where this pattern was observed.
    pub source_context_count: u32,
    /// When this candidate was first observed.
    pub first_seen: DateTime<Utc>,
    /// When this candidate was last observed.
    pub last_seen: DateTime<Utc>,
}

/// Decision on what to do with a trait candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvolutionDecision {
    /// Automatically adopt into the per-user/group soul layer.
    /// (v1: never returned — all candidates go through review.)
    AutoAdopt,
    /// Queue for admin review before adoption.
    QueueForReview,
    /// Insufficient evidence — discard the candidate.
    Discard,
}

/// Evaluates a trait candidate and returns an evolution decision.
///
/// **v1 behavior:** Always returns [`EvolutionDecision::QueueForReview`].
/// Autonomous adoption requires confidence scoring calibration that will
/// be implemented in a future version.
#[must_use]
pub fn evaluate_candidate(_candidate: &TraitCandidate) -> EvolutionDecision {
    // v1: all candidates require review. No autonomous adoption yet.
    EvolutionDecision::QueueForReview
}

/// An audit log entry for a proposed soul layer change.
///
/// All evolution decisions are logged regardless of outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionAuditEntry {
    /// The proposed change.
    pub candidate: TraitCandidate,
    /// Decision taken.
    pub decision: EvolutionDecision,
    /// Reasoning behind the decision.
    pub reasoning: String,
    /// Ed25519 signature of the serialized entry (hex-encoded).
    /// Signed by the agent's identity key for tamper detection.
    pub signature: Option<String>,
    /// When the decision was made.
    pub decided_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_candidate() -> TraitCandidate {
        TraitCandidate {
            observation: "User prefers formal tone".into(),
            confidence_score: 0.85,
            source_context_count: 12,
            first_seen: Utc::now(),
            last_seen: Utc::now(),
        }
    }

    #[test]
    fn v1_always_queues_for_review() {
        let candidate = sample_candidate();
        assert_eq!(
            evaluate_candidate(&candidate),
            EvolutionDecision::QueueForReview
        );
    }

    #[test]
    fn v1_queues_even_high_confidence() {
        let mut candidate = sample_candidate();
        candidate.confidence_score = 1.0;
        candidate.source_context_count = 100;
        assert_eq!(
            evaluate_candidate(&candidate),
            EvolutionDecision::QueueForReview
        );
    }

    #[test]
    fn audit_entry_serialization() {
        let entry = EvolutionAuditEntry {
            candidate: sample_candidate(),
            decision: EvolutionDecision::QueueForReview,
            reasoning: "v1: all candidates queued for review".into(),
            signature: None,
            decided_at: Utc::now(),
        };

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: EvolutionAuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.decision, EvolutionDecision::QueueForReview);
        assert_eq!(
            deserialized.candidate.observation,
            "User prefers formal tone"
        );
    }
}
