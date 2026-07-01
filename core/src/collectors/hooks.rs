//! Reading configured hooks from a `settings.json` file and turning them into
//! [`Watcher`]s. Hooks are the "reactive" watchers — they fire on events.
//!
//! Settings shape:
//! ```json
//! { "hooks": { "Stop": [ { "matcher": "*",
//!     "hooks": [ { "type": "command", "command": "notify.sh" } ] } ] } }
//! ```

use crate::model::{Watcher, WatcherKind};
use serde_json::Value;
use std::path::Path;

/// Read hooks from a settings file. Missing/malformed file → no watchers.
pub fn read_hooks(settings_path: &Path) -> Vec<Watcher> {
    let Ok(text) = std::fs::read_to_string(settings_path) else {
        return Vec::new();
    };
    let Ok(v): Result<Value, _> = serde_json::from_str(&text) else {
        return Vec::new();
    };
    let Some(hooks) = v.get("hooks").and_then(Value::as_object) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for (event, groups) in hooks {
        let Some(groups) = groups.as_array() else {
            continue;
        };
        for group in groups {
            let matcher = group
                .get("matcher")
                .and_then(Value::as_str)
                .unwrap_or("*")
                .to_string();
            let Some(cmds) = group.get("hooks").and_then(Value::as_array) else {
                continue;
            };
            for cmd in cmds {
                let command = cmd
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                out.push(Watcher {
                    kind: WatcherKind::Hook,
                    name: event.clone(),
                    detail: command,
                    schedule: Some(matcher.clone()),
                    last_fired: None,
                    fired_count: 0,
                    next_wake: None,
                    running: true,
                    pid: None,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hooks() {
        let dir = std::env::temp_dir().join(format!("ccw-hooks-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("settings.json");
        std::fs::write(
            &f,
            r#"{"hooks":{"Stop":[{"matcher":"*","hooks":[{"type":"command","command":"notify.sh"}]}]}}"#,
        )
        .unwrap();
        let w = read_hooks(&f);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].name, "Stop");
        assert_eq!(w[0].detail, "notify.sh");
        assert!(matches!(w[0].kind, WatcherKind::Hook));
    }

    #[test]
    fn missing_file_is_empty() {
        assert!(read_hooks(Path::new("/no/such/settings.json")).is_empty());
    }
}
