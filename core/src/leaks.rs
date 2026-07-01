//! Leak-detection heuristics as pure functions over scalar inputs, so each is
//! trivially unit-tested. The engine feeds them windowed rates and assembles
//! the resulting [`crate::model::Alert`]s.

use crate::config::Config;

/// Sustained high burn with no recent user turn → the assistant is looping /
/// talking to itself.
pub fn is_runaway(tokens_per_min: f64, secs_since_user_turn: Option<i64>, cfg: &Config) -> bool {
    if tokens_per_min < cfg.burn_tokens_per_min {
        return false;
    }
    match secs_since_user_turn {
        Some(s) => s >= cfg.runaway_no_user_secs,
        // No user turn on record — we can't distinguish "just started" from
        // "stuck", so don't flag. Requires a *known* stale turn.
        None => false,
    }
}

/// High input rate while the cache-hit ratio has collapsed → paying full input
/// price repeatedly.
pub fn is_cache_bleed(input_per_min: f64, cache_hit_ratio: Option<f64>, cfg: &Config) -> bool {
    if input_per_min < cfg.cache_bleed_min_input_per_min {
        return false;
    }
    match cache_hit_ratio {
        Some(r) => r < cfg.cache_bleed_ratio,
        None => false,
    }
}

/// Session flagged idle but still burning tokens.
pub fn is_zombie(is_idle: bool, tokens_per_min: f64, cfg: &Config) -> bool {
    is_idle && tokens_per_min >= cfg.burn_tokens_per_min
}

/// Too many agents spawned in the storm window.
pub fn is_agent_storm(agent_starts_in_window: usize, cfg: &Config) -> bool {
    agent_starts_in_window >= cfg.agent_storm_count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::default()
    }

    #[test]
    fn runaway_needs_burn_and_no_user() {
        let c = cfg();
        // Burning, no user turn for 6 min → runaway.
        assert!(is_runaway(50_000.0, Some(360), &c));
        // Burning but user was active 10s ago → not runaway.
        assert!(!is_runaway(50_000.0, Some(10), &c));
        // Low burn → never runaway.
        assert!(!is_runaway(1_000.0, Some(1000), &c));
        // Burning but no user turn on record → not enough to flag.
        assert!(!is_runaway(50_000.0, None, &c));
    }

    #[test]
    fn cache_bleed_needs_high_input_and_low_hits() {
        let c = cfg();
        assert!(is_cache_bleed(30_000.0, Some(0.05), &c));
        // Good cache hits → no bleed.
        assert!(!is_cache_bleed(30_000.0, Some(0.8), &c));
        // Low input → no bleed even with poor hits.
        assert!(!is_cache_bleed(1_000.0, Some(0.0), &c));
        // No ratio data → no bleed.
        assert!(!is_cache_bleed(30_000.0, None, &c));
    }

    #[test]
    fn zombie_needs_idle_and_burn() {
        let c = cfg();
        assert!(is_zombie(true, 50_000.0, &c));
        assert!(!is_zombie(false, 50_000.0, &c));
        assert!(!is_zombie(true, 100.0, &c));
    }

    #[test]
    fn agent_storm_threshold() {
        let c = cfg();
        assert!(is_agent_storm(6, &c));
        assert!(is_agent_storm(9, &c));
        assert!(!is_agent_storm(3, &c));
    }
}
