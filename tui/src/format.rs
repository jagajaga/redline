//! Small display formatters shared across the UI.

use chrono::{Local, TimeZone};

/// Compact token count: 1234 → "1.2k", 3_400_000 → "3.4M".
pub fn tokens(n: u64) -> String {
    let f = n as f64;
    if f >= 1_000_000.0 {
        format!("{:.1}M", f / 1_000_000.0)
    } else if f >= 1_000.0 {
        format!("{:.1}k", f / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Compact rate for the tok/min column.
pub fn rate(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.0}k", v / 1_000.0)
    } else {
        format!("{v:.0}")
    }
}

/// Duration between two epoch-ms instants as "2h14m" / "0:40" / "3s".
pub fn ago(then_ms: i64, now_ms: i64) -> String {
    duration_ms((now_ms - then_ms).max(0))
}

pub fn duration_ms(ms: i64) -> String {
    let secs = ms / 1000;
    if secs >= 3600 {
        format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}:{:02}", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

/// Local wall-clock time "14:22:07" from epoch ms.
pub fn clock(ms: i64) -> String {
    match Local.timestamp_millis_opt(ms).single() {
        Some(dt) => dt.format("%H:%M:%S").to_string(),
        None => "--:--:--".to_string(),
    }
}
