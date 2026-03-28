pub const SUPPRESSION_FACTOR: f32 = 0.1;
pub const WINNER_BOOST: f32 = 1.2;
pub const RECENCY_HALF_LIFE_DAYS: f32 = 30.0;

/// Recency score: 2^(-age_days / 30)
pub fn recency_score(age_days: f32) -> f32 {
    let exponent = -age_days / RECENCY_HALF_LIFE_DAYS;
    2.0_f32.powf(exponent)
}

/// Compute effective strength for interference resolution.
pub fn effective_strength(
    activation: f32,
    decay_factor: f32,
    importance: i32,
    age_days: f32,
) -> f32 {
    let importance_factor = (importance as f32 / 10.0).max(0.1).min(2.0);
    let recency = recency_score(age_days);
    activation * decay_factor * importance_factor * recency
}

/// Rough ISO-8601 datetime to approximate unix epoch (seconds).
/// Handles "2024-01-15T10:30:00Z" and similar formats.
pub fn parse_datetime_approx(s: &str) -> f64 {
    // Parse YYYY-MM-DDTHH:MM:SS
    let s = s.trim().trim_end_matches('Z');
    let parts: Vec<&str> = s.split('T').collect();
    if parts.len() < 1 {
        return 0.0;
    }
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    if date_parts.len() < 3 {
        return 0.0;
    }
    let year: f64 = date_parts[0].parse().unwrap_or(1970.0);
    let month: f64 = date_parts[1].parse().unwrap_or(1.0);
    let day: f64 = date_parts[2].parse().unwrap_or(1.0);

    let mut hours: f64 = 0.0;
    let mut mins: f64 = 0.0;
    let mut secs: f64 = 0.0;
    if parts.len() > 1 {
        let time_parts: Vec<&str> = parts[1].split(':').collect();
        hours = time_parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        mins = time_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        secs = time_parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    }

    // Simplified: days since epoch
    let y = year - 1970.0;
    let m_days: f64 = match month as u32 {
        1 => 0.0, 2 => 31.0, 3 => 59.0, 4 => 90.0,
        5 => 120.0, 6 => 151.0, 7 => 181.0, 8 => 212.0,
        9 => 243.0, 10 => 273.0, 11 => 304.0, 12 => 334.0,
        _ => 0.0,
    };
    let total_days = y * 365.25 + m_days + day - 1.0;
    total_days * 86400.0 + hours * 3600.0 + mins * 60.0 + secs
}

/// Current unix epoch approximation (days since 2026-03-27 as reference).
/// Returns days since epoch as f64 for age computation.
pub fn now_unix() -> f64 {
    // 2026-03-27 = approximately 56 years after 1970
    // More precisely: parse from a known reference
    parse_datetime_approx("2026-03-27T00:00:00Z")
}

/// Resolve interference between two memories at contradiction edges.
/// Returns (winner_activation, loser_activation) after adjustment.
pub fn resolve_interference(
    a_activation: f32,
    a_decay: f32,
    a_importance: i32,
    a_age_days: f32,
    b_activation: f32,
    b_decay: f32,
    b_importance: i32,
    b_age_days: f32,
) -> (f32, f32, bool) {
    // Returns (a_new, b_new, a_won)
    let a_eff = effective_strength(a_activation, a_decay, a_importance, a_age_days);
    let b_eff = effective_strength(b_activation, b_decay, b_importance, b_age_days);

    if a_eff >= b_eff {
        // A wins
        let a_new = (a_activation * WINNER_BOOST).min(1.0);
        let b_new = b_activation * SUPPRESSION_FACTOR;
        (a_new, b_new, true)
    } else {
        // B wins
        let a_new = a_activation * SUPPRESSION_FACTOR;
        let b_new = (b_activation * WINNER_BOOST).min(1.0);
        (a_new, b_new, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recency_recent_beats_old() {
        let recent = recency_score(1.0);
        let old = recency_score(90.0);
        assert!(recent > old, "recent={} old={}", recent, old);
        // At age 0: score = 1.0
        let now = recency_score(0.0);
        assert!((now - 1.0).abs() < 1e-5);
        // At half-life (30 days): score = 0.5
        let half = recency_score(30.0);
        assert!((half - 0.5).abs() < 1e-5);
    }

    #[test]
    fn resolve_newer_wins() {
        // Same activation, same importance, A is newer
        let (a_new, b_new, a_won) = resolve_interference(
            0.8, 1.0, 5, 1.0,  // A: 1 day old
            0.8, 1.0, 5, 60.0, // B: 60 days old
        );
        assert!(a_won, "newer memory A should win");
        assert!(a_new > 0.8, "winner gets boost");
        assert!(b_new < 0.8, "loser gets suppressed");
    }

    #[test]
    fn resolve_importance_wins() {
        // Same recency, but B has much higher importance
        let (_, _, a_won) = resolve_interference(
            0.8, 1.0, 3, 10.0,  // A: importance 3
            0.7, 1.0, 9, 10.0,  // B: importance 9
        );
        assert!(!a_won, "higher importance B should win");
    }
}
