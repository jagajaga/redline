//! Pure snapshot → menu-bar presentation logic. Kept free of any GUI toolkit so
//! it's unit-testable; `main.rs` binds this model to `tray-icon`/`muda` items.

use ccwatch_core::model::{Host, SessionState, Snapshot};

/// Compact rate: 1234 → "1k", 3_400_000 → "3.4M".
pub fn rate(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.0}k", v / 1_000.0)
    } else {
        format!("{v:.0}")
    }
}

fn tokens(n: u64) -> String {
    rate(n as f64)
}

fn host_label(h: &Host) -> String {
    match h {
        Host::Local => "local".into(),
        Host::Remote { name, .. } => name.clone(),
        Host::Cloud => "cloud".into(),
    }
}

/// Menu-bar text next to the graph. Alerts dominate; otherwise the current
/// total burn rate. `None` connection state shows a clear "off" marker.
pub fn tray_title(s: &Snapshot, connected: bool) -> String {
    if !connected {
        return "⏻".to_string();
    }
    if !s.alerts.is_empty() {
        format!("⚠{}", s.alerts.len())
    } else {
        rate(s.totals.tokens_per_min)
    }
}

pub fn tooltip(s: &Snapshot) -> String {
    format!(
        "ccwatch — {} active · {} tok/min · {} alerts",
        s.totals.active_sessions,
        rate(s.totals.tokens_per_min),
        s.alerts.len()
    )
}

/// What `main.rs` can do when a session's action item is clicked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionAction {
    /// Local session: signal this pid.
    Signal { pid: i32 },
    /// Remote/cloud session: run the host's cancel command for this id.
    Cancel { remote: String, id: String },
    /// Nothing possible (no pid / no cancel configured).
    None,
}

/// One session's dropdown entry: a submenu title, info lines, and its action.
#[derive(Clone, Debug)]
pub struct SessionEntry {
    pub title: String,
    /// Three informational lines shown inside the submenu.
    pub info: Vec<String>,
    pub name: String,
    pub action: SessionAction,
    /// Label for the destructive action ("Kill session…" / "Cancel on demo-host…").
    pub kill_label: String,
    /// Pause/resume only make sense for local pids.
    pub can_pause: bool,
}

/// The whole dropdown, as pure data.
#[derive(Clone, Debug)]
pub struct MenuModel {
    pub header: String,
    pub alerts: Vec<String>,
    pub sessions: Vec<SessionEntry>,
}

pub fn menu_model(s: &Snapshot) -> MenuModel {
    let header = format!(
        "{} active · {} tok/min · Σ {} · cache {:.0}%",
        s.totals.active_sessions,
        rate(s.totals.tokens_per_min),
        tokens(s.totals.total_tokens),
        s.totals.cache_hit_pct
    );

    let alerts = s
        .alerts
        .iter()
        .map(|a| format!("⚠ {} — {}: {}", a.kind.label(), a.subject, a.message))
        .collect();

    let sessions = s
        .sessions
        .iter()
        .map(|sess| {
            let host = match &sess.host {
                Host::Local => String::new(),
                other => format!("  ·  {}", host_label(other)),
            };
            let state = match sess.state {
                SessionState::Running => "running",
                SessionState::Idle => "idle",
                SessionState::Ended => "ended",
            };
            let t = &sess.tokens;
            let info = vec![
                format!(
                    "{state} · {} · cpu {:.0}% · {} MB",
                    sess.model.as_deref().unwrap_or("-").trim_start_matches("claude-"),
                    sess.cpu_pct,
                    sess.rss_mb
                ),
                format!("cwd {}", sess.cwd),
                format!(
                    "in {} · out {} · cw {} · cr {} · {} msgs",
                    tokens(t.input),
                    tokens(t.output),
                    tokens(t.cache_write),
                    tokens(t.cache_read),
                    t.messages
                ),
            ];
            let local = matches!(sess.host, Host::Local);
            let (action, kill_label) = if local {
                match sess.pid {
                    Some(pid) => (SessionAction::Signal { pid }, "Kill session…".to_string()),
                    None => (SessionAction::None, "No pid — cannot signal".to_string()),
                }
            } else {
                match &sess.remote_name {
                    Some(remote) => (
                        SessionAction::Cancel {
                            remote: remote.clone(),
                            id: sess.id.clone(),
                        },
                        format!("Cancel on {}…", host_label(&sess.host)),
                    ),
                    None => (SessionAction::None, "No cancel command configured".to_string()),
                }
            };
            SessionEntry {
                title: format!("{}  —  {} tok/min{host}", sess.name, rate(sess.tokens_per_min)),
                info,
                name: sess.name.clone(),
                action,
                kill_label,
                can_pause: local && sess.pid.is_some(),
            }
        })
        .collect();

    MenuModel {
        header,
        alerts,
        sessions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ccwatch_core::model::*;

    fn base() -> Snapshot {
        let mut s = Snapshot::empty(0);
        s.totals = Totals {
            active_sessions: 2,
            tokens_per_min: 46_000.0,
            total_tokens: 1_000_000,
            cache_hit_pct: 91.0,
        };
        s
    }

    fn sess(name: &str, tpm: f64, host: Host) -> Session {
        Session {
            id: format!("id-{name}"),
            name: name.into(),
            cwd: "/x".into(),
            pid: Some(4242),
            kind: "interactive".into(),
            entrypoint: "cli".into(),
            version: "1".into(),
            model: Some("claude-opus-4-8".into()),
            state: SessionState::Running,
            started_at: Some(0),
            last_activity: Some(0),
            tokens: TokenLedger::default(),
            tokens_per_min: tpm,
            cpu_pct: 12.0,
            rss_mb: 200,
            agents: vec![],
            tasks: vec![],
            watchers: vec![],
            host,
            remote_name: None,
        }
    }

    #[test]
    fn title_alerts_rate_and_disconnected() {
        let mut s = base();
        assert_eq!(tray_title(&s, true), "46k");
        assert_eq!(tray_title(&s, false), "⏻");
        s.alerts.push(Alert {
            severity: Severity::Critical,
            kind: AlertKind::RunawayLoop,
            subject: "webapp".into(),
            session_id: "b".into(),
            message: "burning".into(),
            since_ms: 0,
        });
        assert_eq!(tray_title(&s, true), "⚠1");
    }

    #[test]
    fn local_session_gets_signal_action_and_pause() {
        let mut s = base();
        s.sessions.push(sess("webapp", 40_000.0, Host::Local));
        let m = menu_model(&s);
        assert_eq!(m.sessions.len(), 1);
        let e = &m.sessions[0];
        assert_eq!(e.action, SessionAction::Signal { pid: 4242 });
        assert!(e.can_pause);
        assert_eq!(e.kill_label, "Kill session…");
        assert!(e.title.contains("webapp"));
        assert!(e.title.contains("40k tok/min"));
        assert_eq!(e.info.len(), 3);
        assert!(e.info[0].contains("running"));
        assert!(e.info[0].contains("opus-4-8"));
        assert!(e.info[1].contains("cwd /x"));
    }

    #[test]
    fn remote_session_gets_cancel_action_no_pause() {
        let mut s = base();
        let mut r = sess(
            "worker",
            6_000.0,
            Host::Remote {
                name: "demo-host".into(),
                ssh_target: "demo-host".into(),
            },
        );
        r.remote_name = Some("demo-host".into());
        s.sessions.push(r);
        let m = menu_model(&s);
        let e = &m.sessions[0];
        assert_eq!(
            e.action,
            SessionAction::Cancel {
                remote: "demo-host".into(),
                id: "id-worker".into()
            }
        );
        assert!(!e.can_pause);
        assert_eq!(e.kill_label, "Cancel on demo-host…");
        assert!(e.title.contains("demo-host"));
    }

    #[test]
    fn remote_without_cancel_config_is_inert() {
        let mut s = base();
        s.sessions.push(sess("cloudy", 1_000.0, Host::Cloud));
        let m = menu_model(&s);
        assert_eq!(m.sessions[0].action, SessionAction::None);
        assert!(!m.sessions[0].can_pause);
    }

    #[test]
    fn header_and_alert_lines() {
        let mut s = base();
        s.alerts.push(Alert {
            severity: Severity::Warn,
            kind: AlertKind::CacheBleed,
            subject: "worker".into(),
            session_id: "w".into(),
            message: "cache-read 12%".into(),
            since_ms: 0,
        });
        let m = menu_model(&s);
        assert!(m.header.contains("2 active"));
        assert!(m.header.contains("46k tok/min"));
        assert_eq!(m.alerts.len(), 1);
        assert!(m.alerts[0].contains("cache bleed"));
        assert!(m.alerts[0].contains("worker"));
    }
}
