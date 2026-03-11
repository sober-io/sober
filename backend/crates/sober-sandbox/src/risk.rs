//! Risk classification for shell commands.

use serde::{Deserialize, Serialize};

/// Risk classification for shell commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// Read-only, informational commands.
    Safe,
    /// Build, install, write operations.
    Moderate,
    /// Destructive or dangerous operations.
    Dangerous,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_level_serde_roundtrip() {
        let variants = [RiskLevel::Safe, RiskLevel::Moderate, RiskLevel::Dangerous];
        for v in variants {
            let json = serde_json::to_string(&v).unwrap();
            let back: RiskLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(v, back);
        }
    }

    #[test]
    fn risk_level_ordering() {
        assert!(RiskLevel::Safe < RiskLevel::Moderate);
        assert!(RiskLevel::Moderate < RiskLevel::Dangerous);
    }
}
