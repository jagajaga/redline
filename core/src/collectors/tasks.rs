//! Reading a session's todo list: `~/.claude/tasks/<sessionId>/*.json`.

use crate::model::Task;
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
struct RawTask {
    #[serde(default)]
    subject: String,
    #[serde(default)]
    status: String,
    #[serde(rename = "activeForm", default)]
    active_form: Option<String>,
    #[serde(rename = "blockedBy", default)]
    blocked_by: Vec<String>,
}

/// Read all tasks for a session, sorted by their numeric filename so display
/// order is stable and matches creation order.
pub fn read_tasks(session_dir: &Path) -> Vec<Task> {
    let mut files: Vec<_> = match std::fs::read_dir(session_dir) {
        Ok(rd) => rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
            .collect(),
        Err(_) => return Vec::new(),
    };
    files.sort_by_key(|p| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(u64::MAX)
    });

    let mut out = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(raw) = serde_json::from_str::<RawTask>(&text) {
            if raw.subject.is_empty() {
                continue;
            }
            out.push(Task {
                subject: raw.subject,
                status: raw.status,
                blocked: !raw.blocked_by.is_empty(),
                active_form: raw.active_form,
            });
        }
    }
    out
}

/// `(completed, total)` for a task list header.
pub fn done_total(tasks: &[Task]) -> (usize, usize) {
    let done = tasks.iter().filter(|t| t.status == "completed").count();
    (done, tasks.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_orders_tasks() {
        let dir = tempdir();
        std::fs::write(
            dir.join("2.json"),
            r#"{"subject":"second","status":"pending","blockedBy":["1"]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("1.json"),
            r#"{"subject":"first","status":"completed","blockedBy":[]}"#,
        )
        .unwrap();
        let tasks = read_tasks(&dir);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].subject, "first");
        assert_eq!(tasks[1].subject, "second");
        assert!(tasks[1].blocked);
        assert_eq!(done_total(&tasks), (1, 2));
    }

    fn tempdir() -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ccw-task-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
