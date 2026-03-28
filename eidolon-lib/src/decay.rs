pub const BASE_DECAY_RATE: f32 = 0.995;
pub const IMPORTANCE_PROTECTION: f32 = 0.002;
pub const DEATH_THRESHOLD: f32 = 0.05;
pub const RECALL_BOOST: f32 = 0.3;
pub const EDGE_DECAY_RATE: f32 = 0.998;

/// Compute new decay factor after ticks.
/// importance_bonus slows decay for important memories.
pub fn compute_pattern_decay(current: f32, ticks: u32, importance: i32) -> f32 {
    // Higher importance = slower decay via IMPORTANCE_PROTECTION bonus
    // Cap at 0.9999 so even maximum importance still decays eventually
    let bonus = IMPORTANCE_PROTECTION * (importance as f32).max(0.0);
    let effective_rate = (BASE_DECAY_RATE + bonus).min(0.9999);
    current * effective_rate.powi(ticks as i32)
}

/// Boost decay factor after recall (memory access).
/// factor + 0.3 * (1 - factor)
pub fn apply_recall_boost(current: f32) -> f32 {
    (current + RECALL_BOOST * (1.0 - current)).min(1.0)
}

/// Classify health of a memory based on decay factor.
pub fn classify_health(decay_factor: f32) -> &'static str {
    if decay_factor >= 0.8 {
        "strong"
    } else if decay_factor >= 0.6 {
        "healthy"
    } else if decay_factor >= 0.4 {
        "fading"
    } else if decay_factor >= DEATH_THRESHOLD {
        "weak"
    } else {
        "dead"
    }
}

pub fn is_dead(decay_factor: f32) -> bool {
    decay_factor < DEATH_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decay_reduces_factor() {
        let initial = 1.0;
        let after = compute_pattern_decay(initial, 100, 5);
        assert!(after < initial, "decay should reduce factor");
        assert!(after > 0.0, "should not reach zero in 100 ticks");
    }

    #[test]
    fn importance_protects() {
        let initial = 0.8;
        let low_imp = compute_pattern_decay(initial, 1000, 1);
        let high_imp = compute_pattern_decay(initial, 1000, 10);
        assert!(high_imp > low_imp, "high importance should decay slower: low={} high={}", low_imp, high_imp);
    }

    #[test]
    fn recall_boost_works() {
        let weak = 0.2;
        let boosted = apply_recall_boost(weak);
        assert!(boosted > weak, "recall boost should increase factor");
        assert!(boosted <= 1.0, "should not exceed 1.0");

        // Already strong: boost is small
        let strong = 0.95;
        let boosted_strong = apply_recall_boost(strong);
        assert!(boosted_strong > strong);
        assert!(boosted_strong <= 1.0);
    }

    #[test]
    fn eventual_death() {
        let mut factor = 1.0;
        for _ in 0..10000 {
            factor = compute_pattern_decay(factor, 1, 1);
        }
        assert!(is_dead(factor) || factor < 0.3, "should decay significantly after 10000 ticks");
    }

    #[test]
    fn classify_health_correct() {
        assert_eq!(classify_health(0.9), "strong");
        assert_eq!(classify_health(0.7), "healthy");
        assert_eq!(classify_health(0.5), "fading");
        assert_eq!(classify_health(0.2), "weak");
        assert_eq!(classify_health(0.03), "dead");
    }

    #[test]
    fn edge_decay_constant() {
        // Verify the constant is accessible and sensible
        assert!(EDGE_DECAY_RATE > 0.9 && EDGE_DECAY_RATE < 1.0);
        let w = 1.0_f32 * EDGE_DECAY_RATE.powi(1000);
        assert!(w > 0.0 && w < 1.0);
    }
}
