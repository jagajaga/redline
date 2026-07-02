//! Tunable thresholds. Sensible defaults ship in code; a `config.toml` under
//! `~/.claude/ccwatch/` can override any field. We parse TOML by hand (simple
//! `key = value` lines) to avoid pulling a TOML dependency into `core`.

use std::path::Path;

#[derive(Clone, Debug)]
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
    /// Governor: plan-window length (Anthropic's session window is 5h).
    pub governor_window_hours: i64,
    /// Governor: plan-window budget (Opus-equivalent tokens). None → learned
    /// from observed 429 walls.
    pub governor_window_budget: Option<u64>,
    /// Governor: weekly budget (Opus-equivalent tokens). None → learned from
    /// weekly markers.
    pub governor_week_budget: Option<u64>,
    /// Per-model weights, normalised to Opus = 1.0. The governor's tanks
    /// accumulate `raw_tokens × weight` (Opus-equivalent units) so the learned
    /// ceiling stays valid as the model mix shifts between wall-hits: 1M Opus
    /// tokens and ~500k Fable tokens (weight 2.0) draw the same. Defaults track
    /// Anthropic's published per-token price ratios — a constant 1:5
    /// input:output ratio across models makes a single scalar per model
    /// well-defined regardless of the input/output split within a message.
    pub weight_opus: f64,
    pub weight_sonnet: f64,
    pub weight_haiku: f64,
    pub weight_fable: f64,
    /// Fallback weight for models that match no known tier.
    pub weight_default: f64,
    /// Terminal app for "Open TUI dashboard" (empty → auto-detect).
    pub terminal_app: String,
}

/// Which weight/cost tier a model string belongs to. Substring match on the
/// lowercased id; `"other"` when nothing matches.
pub fn tier_of(model: Option<&str>) -> &'static str {
    match model {
        Some(m) => {
            let m = m.to_ascii_lowercase();
            if m.contains("opus") {
                "opus"
            } else if m.contains("sonnet") {
                "sonnet"
            } else if m.contains("haiku") {
                "haiku"
            } else if m.contains("fable") || m.contains("mythos") {
                "fable"
            } else {
                "other"
            }
        }
        None => "other",
    }
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
            governor_window_hours: 5,
            governor_window_budget: None,
            governor_week_budget: None,
            // Opus-normalised price ratios: Fable $50/Mtok output vs Opus $25
            // → 2.0; Sonnet $15 → 0.6; Haiku $5 → 0.2 (input ratios coincide).
            weight_opus: 1.0,
            weight_sonnet: 0.6,
            weight_haiku: 0.2,
            weight_fable: 2.0,
            weight_default: 1.0,
            terminal_app: String::new(),
        }
    }
}

impl Config {
    /// Opus-equivalent weight for a model tier (see [`tier_of`]).
    pub fn weight_for(&self, tier: &str) -> f64 {
        match tier {
            "opus" => self.weight_opus,
            "sonnet" => self.weight_sonnet,
            "haiku" => self.weight_haiku,
            "fable" => self.weight_fable,
            _ => self.weight_default,
        }
    }
}

impl Config {
    /// Event retention must cover the governor's window plus slack, whatever
    /// the rate-window retention is set to.
    pub fn retention_secs(&self) -> i64 {
        self.history_retain_secs
            .max(self.governor_window_hours * 3600 + 3600)
            // Weekly tanks need ~9 days of buckets (7-day window + slack so a
            // recent hit's 7-day measurement is fully covered).
            .max(9 * 24 * 3600)
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
                "governor_window_hours" | "window_hours" => {
                    set_i64(&mut cfg.governor_window_hours, v)
                }
                "governor_window_budget" | "window_budget" => {
                    if let Ok(n) = v.replace('_', "").parse() {
                        cfg.governor_window_budget = Some(n);
                    }
                }
                "week_budget" | "governor_week_budget" => {
                    if let Ok(n) = v.replace('_', "").parse() {
                        cfg.governor_week_budget = Some(n);
                    }
                }
                "weight_opus" => set_f64(&mut cfg.weight_opus, v),
                "weight_sonnet" => set_f64(&mut cfg.weight_sonnet, v),
                "weight_haiku" => set_f64(&mut cfg.weight_haiku, v),
                "weight_fable" => set_f64(&mut cfg.weight_fable, v),
                "weight_default" => set_f64(&mut cfg.weight_default, v),
                "terminal_app" | "terminal" => cfg.terminal_app = v.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_and_weight_match_model_ids() {
        assert_eq!(tier_of(Some("claude-opus-4-8")), "opus");
        assert_eq!(tier_of(Some("claude-sonnet-5")), "sonnet");
        assert_eq!(tier_of(Some("claude-haiku-4-5")), "haiku");
        assert_eq!(tier_of(Some("claude-fable-5")), "fable");
        assert_eq!(tier_of(Some("claude-mythos-5")), "fable");
        assert_eq!(tier_of(Some("something-else")), "other");
        assert_eq!(tier_of(None), "other");

        let cfg = Config::default();
        // Opus-normalised price ratios.
        assert_eq!(cfg.weight_for("opus"), 1.0);
        assert_eq!(cfg.weight_for("fable"), 2.0);
        assert_eq!(cfg.weight_for("sonnet"), 0.6);
        assert_eq!(cfg.weight_for("haiku"), 0.2);
        assert_eq!(cfg.weight_for("other"), 1.0);
    }
}
