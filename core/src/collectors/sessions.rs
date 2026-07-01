//! Reading the session registry: `~/.claude/sessions/*.json`. Each file
//! describes one Claude Code process.

use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Debug, Deserialize)]
pub struct SessionMeta {
    #[serde(default)]
    pub pid: Option<i32>,
    #[serde(rename = "sessionId", default)]
    pub session_id: String,
    #[serde(default)]
    pub cwd: String,
    /// Epoch ms.
    #[serde(rename = "startedAt", default)]
    pub started_at: Option<i64>,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub entrypoint: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub name: String,
}

/// Read every session file under `dir`. Unreadable/malformed files are skipped,
/// never fatal.
pub fn read_sessions(dir: &Path) -> Vec<SessionMeta> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(meta) = serde_json::from_str::<SessionMeta>(&text) {
            if !meta.session_id.is_empty() {
                out.push(meta);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_session_file() {
        let dir = tempdir();
        std::fs::write(
            dir.join("39686.json"),
            r#"{"pid":39686,"sessionId":"abc-123","cwd":"/tmp/x","startedAt":1782929280277,"kind":"interactive","entrypoint":"claude-desktop","version":"2.1.197","name":"my-sess"}"#,
        )
        .unwrap();
        let s = read_sessions(&dir);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].pid, Some(39686));
        assert_eq!(s[0].session_id, "abc-123");
        assert_eq!(s[0].name, "my-sess");
    }

    fn tempdir() -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ccw-sess-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
