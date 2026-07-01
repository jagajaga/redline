//! Tunable thresholds. Sensible defaults ship in code; a `config.toml` under
//! `~/.claude/ccwatch/` can override any field. We parse TOML by hand (simple
//! `key = value` lines) to avoid pulling a TOML dependency into `core`.

use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// A session with no activity for longer than this is `Idle`.
    pub idle_secs: i64,
    /// Sliding window (seconds) used for all rate calculations.
    pub rate_window_secs: i64,
    /// tokens/min above which a session is "burning" (runaway / zombie).
    pub burn_tokens_per_min: f64,
    /// Seconds without a user turn before sustained burn is a runaway loop.
    pub runaway_no_user_secs: i64,
    /// Cache-hit ratio below this (with high input) is a cache bleed.
    pub cache_bleed_ratio: f64,
    /// Input tokens/min above which low cache-hit counts as a bleed.
    pub cache_bleed_min_input_per_min: f64,
    /// Number of agents started within `agent_storm_window_secs` that trips the
    /// agent-storm alert.
    pub agent_storm_count: usize,
    pub agent_storm_window_secs: i64,
    /// How long parsed transcript events are retained for rate windows.
    pub history_retain_secs: i64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            idle_secs: 120,
            rate_window_secs: 300,
            burn_tokens_per_min: 40_000.0,
            runaway_no_user_secs: 300,
            cache_bleed_ratio: 0.2,
            cache_bleed_min_input_per_min: 20_000.0,
            agent_storm_count: 6,
            agent_storm_window_secs: 60,
            history_retain_secs: 1800,
        }
    }
}

impl Config {
    /// Load overrides from a `key = value` TOML-ish file, falling back to
    /// defaults for anything absent or unparseable. Missing file → defaults.
    pub fn load(path: &Path) -> Config {
        let mut cfg = Config::default();
        let Ok(text) = std::fs::read_to_string(path) else {
            return cfg;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let k = k.trim();
            let v = v.trim().trim_matches('"');
            match k {
                "idle_secs" => set_i64(&mut cfg.idle_secs, v),
                "rate_window_secs" => set_i64(&mut cfg.rate_window_secs, v),
                "burn_tokens_per_min" => set_f64(&mut cfg.burn_tokens_per_min, v),
                "runaway_no_user_secs" => set_i64(&mut cfg.runaway_no_user_secs, v),
                "cache_bleed_ratio" => set_f64(&mut cfg.cache_bleed_ratio, v),
                "cache_bleed_min_input_per_min" => {
                    set_f64(&mut cfg.cache_bleed_min_input_per_min, v)
                }
                "agent_storm_count" => set_usize(&mut cfg.agent_storm_count, v),
                "agent_storm_window_secs" => set_i64(&mut cfg.agent_storm_window_secs, v),
                "history_retain_secs" => set_i64(&mut cfg.history_retain_secs, v),
                _ => {}
            }
        }
        cfg
    }
}

fn set_i64(target: &mut i64, v: &str) {
    if let Ok(n) = v.parse() {
        *target = n;
    }
}
fn set_usize(target: &mut usize, v: &str) {
    if let Ok(n) = v.parse() {
        *target = n;
    }
}
fn set_f64(target: &mut f64, v: &str) {
    if let Ok(n) = v.parse() {
        *target = n;
    }
}
