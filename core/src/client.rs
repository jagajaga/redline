//! Shared daemon client: ensure `ccwatchd` is up, stream snapshots over the
//! Unix socket, and send one-shot actions. The menu-bar, TUI, and dashboard all
//! talk to the daemon the same way — this is the one blessed path (previously
//! duplicated verbatim in each client).

use crate::ipc::{ActionRequest, ClientMsg, ServerMsg};
use crate::model::Snapshot;
use crate::Paths;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// Messages surfaced from the daemon reader thread to a UI loop. `Heartbeat`
/// and `Disconnected` let a client show liveness / trigger a retry; clients that
/// only care about state can use [`subscribe_snapshots`] instead.
pub enum FromDaemon {
    Snapshot(Box<Snapshot>),
    Heartbeat,
    Disconnected,
}

/// Ensure a daemon is reachable: connect if one is up, else spawn `ccwatchd`
/// (found next to this executable, or on PATH) and wait for its socket.
pub fn ensure_daemon(paths: &Paths) -> anyhow::Result<()> {
    if UnixStream::connect(paths.socket()).is_ok() {
        return Ok(());
    }
    let bin = daemon_binary();
    std::process::Command::new(&bin)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("could not spawn daemon {bin:?}: {e}"))?;

    // Wait up to ~4s for the socket to come alive.
    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline {
        if UnixStream::connect(paths.socket()).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("daemon did not come up at {}", paths.socket().display())
}

fn daemon_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("ccwatchd");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    std::path::PathBuf::from("ccwatchd")
}

/// Subscribe and stream every daemon message on a background thread. The thread
/// signals `Disconnected` on any read failure so the UI can retry.
pub fn subscribe(paths: &Paths) -> anyhow::Result<Receiver<FromDaemon>> {
    let stream = UnixStream::connect(paths.socket())?;
    let (tx, rx) = mpsc::channel();
    let mut writer = stream.try_clone()?;
    let reader = BufReader::new(stream);

    let sub = serde_json::to_string(&ClientMsg::Subscribe)? + "\n";
    writer.write_all(sub.as_bytes())?;
    writer.flush()?;

    std::thread::spawn(move || {
        for line in reader.lines() {
            let Ok(line) = line else {
                let _ = tx.send(FromDaemon::Disconnected);
                return;
            };
            match serde_json::from_str::<ServerMsg>(&line) {
                Ok(ServerMsg::Snapshot(s)) => {
                    if tx.send(FromDaemon::Snapshot(s)).is_err() {
                        return;
                    }
                }
                Ok(ServerMsg::Heartbeat { .. }) => {
                    if tx.send(FromDaemon::Heartbeat).is_err() {
                        return;
                    }
                }
                Ok(ServerMsg::ActionResult { .. }) => {}
                Err(_) => {}
            }
        }
        let _ = tx.send(FromDaemon::Disconnected);
    });

    Ok(rx)
}

/// Subscribe, forwarding only snapshots (heartbeats/disconnects dropped) — for
/// clients that only render state.
pub fn subscribe_snapshots(paths: &Paths) -> anyhow::Result<Receiver<Snapshot>> {
    let inner = subscribe(paths)?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(msg) = inner.recv() {
            if let FromDaemon::Snapshot(s) = msg {
                if tx.send(*s).is_err() {
                    return;
                }
            }
        }
    });
    Ok(rx)
}

/// Send an action on a fresh connection and return `(ok, message)`.
pub fn send_action(paths: &Paths, req: ActionRequest) -> (bool, String) {
    let inner = || -> anyhow::Result<(bool, String)> {
        let mut stream = UnixStream::connect(paths.socket())?;
        let line = serde_json::to_string(&ClientMsg::Action(req.clone()))? + "\n";
        stream.write_all(line.as_bytes())?;
        stream.flush()?;
        let mut reader = BufReader::new(stream);
        let mut resp = String::new();
        reader.read_line(&mut resp)?;
        match serde_json::from_str::<ServerMsg>(resp.trim())? {
            ServerMsg::ActionResult { ok, message } => Ok((ok, message)),
            _ => Ok((false, "unexpected daemon response".into())),
        }
    };
    inner().unwrap_or_else(|e| (false, format!("action failed: {e}")))
}

/// Block for the first snapshot, then keep the latest that arrives within
/// `settle` (so remote hosts, fetched on a slower cadence, have time to merge
/// in). Used by one-shot dumps.
pub fn latest_snapshot(
    paths: &Paths,
    initial_timeout: Duration,
    settle: Duration,
) -> anyhow::Result<Snapshot> {
    let rx = subscribe_snapshots(paths)?;
    let mut snap = rx
        .recv_timeout(initial_timeout)
        .map_err(|_| anyhow::anyhow!("no snapshot within {initial_timeout:?}"))?;
    let deadline = Instant::now() + settle;
    while let Ok(next) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        snap = next;
    }
    Ok(snap)
}
