//! Minimal daemon client for the menu-bar: ensure a daemon is up, then stream
//! snapshots (heartbeats are dropped — the menu bar only cares about state).

use ccwatch_core::ipc::{ClientMsg, ServerMsg};
use ccwatch_core::model::Snapshot;
use ccwatch_core::Paths;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

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

/// Subscribe; a background thread forwards each snapshot on the channel.
pub fn subscribe(paths: &Paths) -> anyhow::Result<Receiver<Snapshot>> {
    let stream = UnixStream::connect(paths.socket())?;
    let mut writer = stream.try_clone()?;
    let reader = BufReader::new(stream);
    writer.write_all((serde_json::to_string(&ClientMsg::Subscribe)? + "\n").as_bytes())?;
    writer.flush()?;

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in reader.lines() {
            let Ok(line) = line else { return };
            if let Ok(ServerMsg::Snapshot(s)) = serde_json::from_str::<ServerMsg>(&line) {
                if tx.send(*s).is_err() {
                    return;
                }
            }
        }
    });
    Ok(rx)
}

/// Block for the first snapshot, then keep the latest that arrives within
/// `settle` (so remote hosts, fetched on a slower cadence, have time to merge
/// in). Used by `--dump`.
pub fn latest_snapshot(
    paths: &Paths,
    initial_timeout: Duration,
    settle: Duration,
) -> anyhow::Result<Snapshot> {
    let rx = subscribe(paths)?;
    let mut snap = rx
        .recv_timeout(initial_timeout)
        .map_err(|_| anyhow::anyhow!("no snapshot within {initial_timeout:?}"))?;
    let deadline = Instant::now() + settle;
    while let Ok(next) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        snap = next;
    }
    Ok(snap)
}
