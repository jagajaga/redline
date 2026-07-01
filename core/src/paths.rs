//! Locating the Claude data root and its sub-directories. Everything is derived
//! from one `root` (normally `~/.claude`) so tests can point at a fixture tree.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Paths { root: root.into() }
    }

    /// `~/.claude`, honoring `$CLAUDE_CONFIG_DIR` then `$HOME`.
    pub fn discover() -> Self {
        if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
            return Paths::new(dir);
        }
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Paths::new(home.join(".claude"))
    }

    pub fn sessions(&self) -> PathBuf {
        self.root.join("sessions")
    }
    pub fn tasks(&self) -> PathBuf {
        self.root.join("tasks")
    }
    pub fn projects(&self) -> PathBuf {
        self.root.join("projects")
    }
    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    /// Per-session task directory.
    pub fn tasks_for(&self, session_id: &str) -> PathBuf {
        self.tasks().join(session_id)
    }
    /// The ccwatch working dir (socket, pidfile, config, history).
    pub fn ccwatch_dir(&self) -> PathBuf {
        self.root.join("ccwatch")
    }
    pub fn socket(&self) -> PathBuf {
        self.ccwatch_dir().join("daemon.sock")
    }
    pub fn pidfile(&self) -> PathBuf {
        self.ccwatch_dir().join("daemon.pid")
    }
    pub fn config_file(&self) -> PathBuf {
        self.ccwatch_dir().join("config.toml")
    }
    /// Remote/cloud host definitions (JSON array of `RemoteDef`).
    pub fn remotes_file(&self) -> PathBuf {
        self.ccwatch_dir().join("remotes.json")
    }
    pub fn action_log(&self) -> PathBuf {
        self.ccwatch_dir().join("actions.log")
    }
}

/// Project-scoped settings file, e.g. `<cwd>/.claude/settings.json`.
pub fn project_settings(cwd: &Path) -> PathBuf {
    cwd.join(".claude").join("settings.json")
}
