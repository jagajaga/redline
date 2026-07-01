//! Data model shared across the engine, daemon IPC, and clients.
//!
//! Every entity is tagged (directly or via its owning session) with a [`Host`]
//! so a future remote/cloud phase can group by machine without reshaping this
//! model.

use serde::{Deserialize, Serialize};

/// Where an entity runs. Phase 1 only ever produces [`Host::Local`]; the other
/// variants exist so Phase 2 (SSH / cloud) is additive.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Host {
    #[default]
    Local,
    Remote { name: String, ssh_target: String },
    Cloud,
}

impl Host {
    pub fn label(&self) -> String {
        match self {
            Host::Local => "local".to_string(),
            Host::Remote { name, .. } => name.clone(),
            Host::Cloud => "cloud".to_string(),
        }
    }
}

/// Raw token counters, kept separate (never collapsed) so cache behaviour is
/// visible. `cw` = cache-write (`cache_creation_input_tokens`), `cr` =
/// cache-read (`cache_read_input_tokens`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenLedger {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub web_search: u64,
    pub web_fetch: u64,
    /// Number of assistant messages folded into this ledger.
    pub messages: u64,
}

impl TokenLedger {
    /// Fold another ledger into this one.
    pub fn add(&mut self, other: &TokenLedger) {
        self.input += other.input;
        self.output += other.output;
        self.cache_write += other.cache_write;
        self.cache_read += other.cache_read;
        self.web_search += other.web_search;
        self.web_fetch += other.web_fetch;
        self.messages += other.messages;
    }

    /// Every token the session touched, including cheap cache reads. Used for
    /// the cumulative "total tokens" display, not for burn rate.
    pub fn grand_total(&self) -> u64 {
        self.input + self.output + self.cache_write + self.cache_read
    }

    /// Tokens that reflect real spend/work: fresh input, output, and cache
    /// writes. Excludes cache reads (huge in volume but cheap), so the burn
    /// rate isn't dominated by a well-functioning cache.
    pub fn billable(&self) -> u64 {
        self.input + self.output + self.cache_write
    }

    /// Fraction of input-side tokens served from cache, in `0.0..=1.0`.
    /// Returns `None` when there is no input-side traffic yet.
    pub fn cache_hit_ratio(&self) -> Option<f64> {
        let denom = self.input + self.cache_write + self.cache_read;
        if denom == 0 {
            None
        } else {
            Some(self.cache_read as f64 / denom as f64)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// pid alive and activity within the idle threshold.
    Running,
    /// pid alive but no recent activity.
    Idle,
    /// pid gone. Never displayed; retained only transiently in the engine.
    Ended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Running,
    Finished,
}

/// A subagent invocation detected from an `Agent`/`Task`/`Workflow` tool call.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Agent {
    /// The originating `tool_use` id (`toolu_…`).
    pub id: String,
    pub subagent_type: String,
    pub description: String,
    pub model: Option<String>,
    pub state: AgentState,
    /// Transcript timestamp (ms epoch) of the launching tool call.
    pub started_at: Option<i64>,
    /// Best-effort tokens attributed to this agent (0 when the subagent runs
    /// out-of-transcript, which is common).
    pub tokens: TokenLedger,
    pub tokens_per_min: f64,
    /// Nested agents this agent spawned.
    pub children: Vec<Agent>,
}

/// A todo item from `~/.claude/tasks/<sessionId>/*.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub subject: String,
    pub status: String,
    pub blocked: bool,
    pub active_form: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatcherKind {
    Hook,
    Loop,
    Routine,
    Background,
}

/// Anything that fires repeatedly or reactively.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Watcher {
    pub kind: WatcherKind,
    /// Short display name (hook event, loop label, command name).
    pub name: String,
    /// The command / target / prompt behind it.
    pub detail: String,
    /// Interval or cron expression, when known.
    pub schedule: Option<String>,
    pub last_fired: Option<i64>,
    pub fired_count: u64,
    pub next_wake: Option<i64>,
    pub running: bool,
    pub pid: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warn,
    Critical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    RunawayLoop,
    CacheBleed,
    ZombieSession,
    AgentStorm,
    StuckWatcher,
}

impl AlertKind {
    pub fn label(&self) -> &'static str {
        match self {
            AlertKind::RunawayLoop => "runaway loop",
            AlertKind::CacheBleed => "cache bleed",
            AlertKind::ZombieSession => "zombie session",
            AlertKind::AgentStorm => "agent storm",
            AlertKind::StuckWatcher => "stuck watcher",
        }
    }
}

/// A detected problem attributed to a specific entity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Alert {
    pub severity: Severity,
    pub kind: AlertKind,
    /// Human-facing name of the culprit (session name, agent desc, …).
    pub subject: String,
    /// Session id the alert belongs to, for jump/act.
    pub session_id: String,
    pub message: String,
    /// Epoch ms since which the condition has held.
    pub since_ms: i64,
}

/// One running/idle session with everything hanging off it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub cwd: String,
    pub pid: Option<i32>,
    pub kind: String,
    pub entrypoint: String,
    pub version: String,
    pub model: Option<String>,
    pub state: SessionState,
    pub started_at: Option<i64>,
    pub last_activity: Option<i64>,
    pub tokens: TokenLedger,
    pub tokens_per_min: f64,
    pub cpu_pct: f32,
    pub rss_mb: u64,
    pub agents: Vec<Agent>,
    pub tasks: Vec<Task>,
    pub watchers: Vec<Watcher>,
    pub host: Host,
}

/// Aggregate figures for the top bar.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Totals {
    pub active_sessions: usize,
    pub tokens_per_min: f64,
    pub total_tokens: u64,
    /// Aggregate cache-hit percentage in `0.0..=100.0`.
    pub cache_hit_pct: f64,
}

/// The full picture at one instant — what the daemon pushes to clients.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    /// Epoch ms the snapshot was generated.
    pub generated_at: i64,
    pub sessions: Vec<Session>,
    pub alerts: Vec<Alert>,
    pub totals: Totals,
}

impl Snapshot {
    pub fn empty(generated_at: i64) -> Self {
        Snapshot {
            generated_at,
            sessions: Vec::new(),
            alerts: Vec::new(),
            totals: Totals::default(),
        }
    }
}
