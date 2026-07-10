//! Executing actions on what the dashboard finds. Kept in `core` (not the
//! daemon) so the logic is reusable and unit-testable. Signals are sent via the
//! `kill` command to avoid a libc/nix dependency.

use std::process::Command;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionOutcome {
    Ok(String),
    Failed(String),
}

/// Send `signal` (e.g. "TERM", "KILL", "STOP", "CONT", "0") to `pid`.
/// Returns whether the kill call reported success.
pub fn signal(pid: i32, sig: &str) -> bool {
    if pid <= 0 {
        return false;
    }
    Command::new("kill")
        .arg(format!("-{sig}"))
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Is the pid alive? (`kill -0`.)
pub fn alive(pid: i32) -> bool {
    signal(pid, "0")
}

/// Freeze a process without killing it.
pub fn pause(pid: i32) -> ActionOutcome {
    if signal(pid, "STOP") {
        ActionOutcome::Ok(format!("paused pid {pid} (SIGSTOP)"))
    } else {
        ActionOutcome::Failed(format!("could not pause pid {pid}"))
    }
}

/// Resume a paused process.
pub fn resume(pid: i32) -> ActionOutcome {
    if signal(pid, "CONT") {
        ActionOutcome::Ok(format!("resumed pid {pid} (SIGCONT)"))
    } else {
        ActionOutcome::Failed(format!("could not resume pid {pid}"))
    }
}

/// Build the `ssh` argv to send `signal` to `pid` on `ssh_target`. Kept pure so
/// it can be unit-tested without a network. `BatchMode=yes` fails fast instead of
/// prompting for a password, and `ConnectTimeout` bounds how long an unreachable
/// host can stall the caller (the refresher runs actuations synchronously). The
/// pid is a plain integer and the target comes from trusted remote config, so
/// there's no shell to inject into — every piece is a separate argv element.
pub fn ssh_signal_args(ssh_target: &str, pid: i32, sig: &str) -> Vec<String> {
    vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ConnectTimeout=5".into(),
        ssh_target.into(),
        "kill".into(),
        format!("-{sig}"),
        pid.to_string(),
    ]
}

/// Send `signal` to `pid` on a remote host over SSH. Returns whether ssh+kill
/// reported success (a network/auth failure is a `false`, never a panic).
pub fn signal_remote(ssh_target: &str, pid: i32, sig: &str) -> bool {
    if pid <= 0 || ssh_target.is_empty() {
        return false;
    }
    Command::new("ssh")
        .args(ssh_signal_args(ssh_target, pid, sig))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Freeze a remote process (`ssh <target> kill -STOP <pid>`).
pub fn pause_remote(ssh_target: &str, pid: i32) -> ActionOutcome {
    if signal_remote(ssh_target, pid, "STOP") {
        ActionOutcome::Ok(format!("paused pid {pid} on {ssh_target} (SIGSTOP)"))
    } else {
        ActionOutcome::Failed(format!("could not pause pid {pid} on {ssh_target}"))
    }
}

/// Resume a remote process (`ssh <target> kill -CONT <pid>`).
pub fn resume_remote(ssh_target: &str, pid: i32) -> ActionOutcome {
    if signal_remote(ssh_target, pid, "CONT") {
        ActionOutcome::Ok(format!("resumed pid {pid} on {ssh_target} (SIGCONT)"))
    } else {
        ActionOutcome::Failed(format!("could not resume pid {pid} on {ssh_target}"))
    }
}

/// Is a remote pid alive? (`ssh <target> kill -0 <pid>`.) An unreachable host
/// reads as not-alive — callers must treat that as "can't confirm", not "dead".
pub fn alive_remote(ssh_target: &str, pid: i32) -> bool {
    signal_remote(ssh_target, pid, "0")
}

/// Terminate a background command (single SIGTERM).
pub fn kill_background(pid: i32) -> ActionOutcome {
    if signal(pid, "TERM") {
        ActionOutcome::Ok(format!("sent SIGTERM to pid {pid}"))
    } else {
        ActionOutcome::Failed(format!("could not signal pid {pid}"))
    }
}

/// Terminate a session: SIGTERM, wait `grace`, then SIGKILL if still alive.
pub fn terminate_session(pid: i32, grace: Duration) -> ActionOutcome {
    if !signal(pid, "TERM") {
        return ActionOutcome::Failed(format!("could not signal pid {pid}"));
    }
    std::thread::sleep(grace);
    if alive(pid) {
        if signal(pid, "KILL") {
            ActionOutcome::Ok(format!("pid {pid}: SIGTERM then SIGKILL"))
        } else {
            ActionOutcome::Failed(format!("pid {pid} survived SIGTERM and SIGKILL failed"))
        }
    } else {
        ActionOutcome::Ok(format!("pid {pid} exited on SIGTERM"))
    }
}

/// Remove a hook (matched by event + command) from a settings.json file,
/// backing up the original to `<file>.bak` first. Reversible by restoring the
/// backup.
pub fn disable_hook(settings_path: &std::path::Path, event: &str, command: &str) -> ActionOutcome {
    let text = match std::fs::read_to_string(settings_path) {
        Ok(t) => t,
        Err(e) => return ActionOutcome::Failed(format!("read {settings_path:?}: {e}")),
    };
    let mut root: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return ActionOutcome::Failed(format!("parse {settings_path:?}: {e}")),
    };

    let mut removed = false;
    if let Some(groups) = root
        .get_mut("hooks")
        .and_then(|h| h.get_mut(event))
        .and_then(|g| g.as_array_mut())
    {
        for group in groups.iter_mut() {
            if let Some(cmds) = group.get_mut("hooks").and_then(|c| c.as_array_mut()) {
                let before = cmds.len();
                cmds.retain(|c| c.get("command").and_then(|v| v.as_str()) != Some(command));
                if cmds.len() != before {
                    removed = true;
                }
            }
        }
        groups.retain(|g| {
            g.get("hooks")
                .and_then(|c| c.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(true)
        });
    }

    if !removed {
        return ActionOutcome::Failed(format!("hook '{event}' → '{command}' not found"));
    }

    let backup = settings_path.with_extension("json.bak");
    if let Err(e) = std::fs::write(&backup, &text) {
        return ActionOutcome::Failed(format!("write backup {backup:?}: {e}"));
    }
    let pretty = serde_json::to_string_pretty(&root).unwrap_or(text);
    if let Err(e) = std::fs::write(settings_path, pretty) {
        return ActionOutcome::Failed(format!("write {settings_path:?}: {e}"));
    }
    ActionOutcome::Ok(format!(
        "disabled hook '{event}' → '{command}' (backup at {backup:?})"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_resume_kill_real_child() {
        // Spawn a harmless long sleep and drive it through the lifecycle.
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id() as i32;
        assert!(alive(pid));
        assert!(matches!(pause(pid), ActionOutcome::Ok(_)));
        assert!(matches!(resume(pid), ActionOutcome::Ok(_)));
        let out = terminate_session(pid, Duration::from_millis(300));
        assert!(matches!(out, ActionOutcome::Ok(_)), "got {out:?}");
        let _ = child.wait();
        assert!(!alive(pid));
    }

    #[test]
    fn ssh_signal_args_are_separate_argv_elements() {
        // Every piece is its own argv element (no shell string to inject into),
        // BatchMode + ConnectTimeout are present, and the pid/signal are last.
        let args = ssh_signal_args("user@host", 4242, "STOP");
        assert_eq!(
            args,
            vec![
                "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "user@host", "kill", "-STOP",
                "4242"
            ]
        );
    }

    #[test]
    fn remote_signal_rejects_bad_input() {
        // Guard rails: a non-positive pid or empty target never spawns ssh.
        assert!(!signal_remote("host", 0, "STOP"));
        assert!(!signal_remote("host", -1, "STOP"));
        assert!(!signal_remote("", 100, "STOP"));
    }

    #[test]
    fn disable_hook_removes_and_backs_up() {
        let dir = std::env::temp_dir().join(format!("ccw-act-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("settings.json");
        std::fs::write(
            &f,
            r#"{"model":"x","hooks":{"Stop":[{"matcher":"*","hooks":[{"type":"command","command":"notify.sh"}]}]}}"#,
        )
        .unwrap();
        let out = disable_hook(&f, "Stop", "notify.sh");
        assert!(matches!(out, ActionOutcome::Ok(_)), "got {out:?}");
        let after = std::fs::read_to_string(&f).unwrap();
        assert!(!after.contains("notify.sh"));
        assert!(after.contains("\"model\""));
        assert!(dir.join("settings.json.bak").exists());
        // Removing again fails cleanly.
        assert!(matches!(
            disable_hook(&f, "Stop", "notify.sh"),
            ActionOutcome::Failed(_)
        ));
    }
}
