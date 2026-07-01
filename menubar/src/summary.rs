//! Pure snapshot → menu-bar presentation logic. Kept free of any GUI toolkit so
//! it's unit-testable; `main.rs` just feeds these strings to `tray-icon`.

use ccwatch_core::model::{Host, Snapshot};

/// Compact rate: 1234 → "1k", 3_400_000 → "3.4M".
fn rate(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.0}k", v / 1_000.0)
    } else {
        format!("{v:.0}")
    }
}

fn host_label(h: &Host) -> String {
    match h {
        Host::Local => "local".into(),
        Host::Remote { name, .. } => name.clone(),
        Host::Cloud => "cloud".into(),
    }
}

/// The text shown in the menu bar itself. Alerts take priority — a leak should
/// be visible at a glance without opening the menu.
pub fn tray_title(s: &Snapshot) -> String {
    if !s.alerts.is_empty() {
        format!("⚠{}", s.alerts.len())
    } else {
        format!("⚡{} · {}", s.totals.active_sessions, rate(s.totals.tokens_per_min))
    }
}

/// Hover tooltip with the fuller picture.
pub fn tooltip(s: &Snapshot) -> String {
    format!(
        "ccwatch — {} active · {} tok/min · {} alerts",
        s.totals.active_sessions,
        rate(s.totals.tokens_per_min),
        s.alerts.len()
    )
}

/// The lines of the dropdown menu, top to bottom (excluding the Quit item).
pub fn menu_lines(s: &Snapshot) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(
        "{} active · {} tok/min · cache {:.0}%",
        s.totals.active_sessions,
        rate(s.totals.tokens_per_min),
        s.totals.cache_hit_pct
    ));
    for a in &s.alerts {
        lines.push(format!("⚠ {} — {}", a.kind.label(), a.subject));
    }
    for sess in &s.sessions {
        let host = match &sess.host {
            Host::Local => String::new(),
            other => format!(" [{}]", host_label(other)),
        };
        lines.push(format!(
            "{} · {} tok/min{}",
            sess.name,
            rate(sess.tokens_per_min),
            host
        ));
    }
    lines
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
            id: name.into(),
            name: name.into(),
            cwd: "/x".into(),
            pid: Some(1),
            kind: "interactive".into(),
            entrypoint: "cli".into(),
            version: "1".into(),
            model: None,
            state: SessionState::Running,
            started_at: Some(0),
            last_activity: Some(0),
            tokens: TokenLedger::default(),
            tokens_per_min: tpm,
            cpu_pct: 0.0,
            rss_mb: 0,
            agents: vec![],
            tasks: vec![],
            watchers: vec![],
            host,
            remote_name: None,
        }
    }

    #[test]
    fn title_shows_alert_count_when_present() {
        let mut s = base();
        s.alerts.push(Alert {
            severity: Severity::Critical,
            kind: AlertKind::RunawayLoop,
            subject: "webapp".into(),
            session_id: "b".into(),
            message: "burning".into(),
            since_ms: 0,
        });
        assert_eq!(tray_title(&s), "⚠1");
    }

    #[test]
    fn title_shows_counts_when_no_alerts() {
        let s = base();
        assert_eq!(tray_title(&s), "⚡2 · 46k");
    }

    #[test]
    fn menu_lists_header_alerts_and_sessions_with_host() {
        let mut s = base();
        s.alerts.push(Alert {
            severity: Severity::Warn,
            kind: AlertKind::CacheBleed,
            subject: "worker".into(),
            session_id: "w".into(),
            message: "".into(),
            since_ms: 0,
        });
        s.sessions.push(sess("webapp", 40_000.0, Host::Local));
        s.sessions.push(sess(
            "worker",
            6_000.0,
            Host::Remote {
                name: "demo-host".into(),
                ssh_target: "demo-host".into(),
            },
        ));
        let lines = menu_lines(&s);
        assert!(lines[0].contains("2 active"));
        assert!(lines.iter().any(|l| l.contains("⚠") && l.contains("cache bleed")));
        assert!(lines.iter().any(|l| l == "webapp · 40k tok/min"));
        assert!(lines.iter().any(|l| l.contains("worker · 6k tok/min [demo-host]")));
    }
}
