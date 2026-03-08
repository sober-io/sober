//! Importance scoring — pure functions for memory decay, boost, and pruning.

/// Computes the decayed importance score using exponential decay.
///
/// Formula: `base * 0.5^(elapsed_days / half_life_days)`
///
/// Returns 0.0 if `half_life_days` is 0 to avoid division by zero.
#[must_use]
pub fn decay(base_importance: f64, elapsed_days: f64, half_life_days: u32) -> f64 {
    if half_life_days == 0 {
        return 0.0;
    }
    let exponent = elapsed_days / f64::from(half_life_days);
    let factor = 0.5_f64.powf(exponent);
    (base_importance * factor).max(0.0)
}

/// Applies a retrieval boost to the current importance, capped at 1.0.
#[must_use]
pub fn boost(current: f64, retrieval_boost: f64) -> f64 {
    (current + retrieval_boost).clamp(0.0, 1.0)
}

/// Returns `true` if the score is below the pruning threshold.
#[must_use]
pub fn should_prune(score: f64, threshold: f64) -> bool {
    score < threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decay_at_zero_days_returns_full_score() {
        let result = decay(1.0, 0.0, 30);
        assert!((result - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn decay_at_half_life_returns_half_score() {
        let result = decay(1.0, 30.0, 30);
        assert!((result - 0.5).abs() < 0.001);
    }

    #[test]
    fn decay_at_two_half_lives_returns_quarter() {
        let result = decay(1.0, 60.0, 30);
        assert!((result - 0.25).abs() < 0.001);
    }

    #[test]
    fn decay_with_base_less_than_one() {
        let result = decay(0.8, 30.0, 30);
        assert!((result - 0.4).abs() < 0.001);
    }

    #[test]
    fn decay_never_goes_negative() {
        let result = decay(1.0, 10000.0, 30);
        assert!(result >= 0.0);
    }

    #[test]
    fn decay_zero_half_life_returns_zero() {
        let result = decay(1.0, 10.0, 0);
        assert!((result - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn boost_adds_to_current() {
        let result = boost(0.5, 0.2);
        assert!((result - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn boost_caps_at_one() {
        let result = boost(0.9, 0.2);
        assert!((result - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn boost_zero_delta_is_noop() {
        let result = boost(0.5, 0.0);
        assert!((result - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn should_prune_below_threshold() {
        assert!(should_prune(0.05, 0.1));
    }

    #[test]
    fn should_not_prune_at_threshold() {
        assert!(!should_prune(0.1, 0.1));
    }

    #[test]
    fn should_not_prune_above_threshold() {
        assert!(!should_prune(0.5, 0.1));
    }
}
