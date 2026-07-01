//! `ccwatchd` — the always-on collector daemon.
//!
//! It owns the single [`Engine`], refreshes it on file-change events (with a
//! poll backstop), and serves newline-delimited JSON over a Unix domain socket:
//! subscribers get pushed snapshots; action requests are executed and answered.
//!
//! `ccwatchd --once` prints a single snapshot as JSON and exits — handy for
//! scripting and verification.

mod remotes;

use ccwatch_core::actions::{self, ActionOutcome};
use ccwatch_core::ipc::{ActionRequest, ClientMsg, ServerMsg};
use ccwatch_core::model::Snapshot;
use ccwatch_core::remote::{self, RemoteDef, SystemRunner};
use ccwatch_core::{Config, Engine, Paths};
use remotes::RemoteManager;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, RwLock};
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(1500);
const PUSH_INTERVAL: Duration = Duration::from_millis(500);
const KILL_GRACE: Duration = Duration::from_secs(2);
/// How often to re-fetch remote hosts (override with `CCWATCH_REMOTE_SECS`).
const DEFAULT_REMOTE_SECS: u64 = 15;

type SharedSnapshot = Arc<RwLock<Arc<Snapshot>>>;

fn main() -> anyhow::Result<()> {
    let paths = Paths::discover();

    if std::env::args().any(|a| a == "--once") {
        let mut engine = Engine::with_defaults();
        let snap = engine.refresh_now();
        println!("{}", serde_json::to_string_pretty(&snap)?);
        return Ok(());
    }

    std::fs::create_dir_all(paths.ccwatch_dir())?;

    // Refuse to double-launch.
    if let Some(pid) = read_pidfile(&paths) {
        if actions::alive(pid) {
            eprintln!("ccwatchd already running (pid {pid})");
            return Ok(());
        }
    }
    std::fs::write(paths.pidfile(), std::process::id().to_string())?;

    // Fresh socket.
    let sock = paths.socket();
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock)?;
    eprintln!("ccwatchd listening at {}", sock.display());

    // Shared latest snapshot, updated by the refresher thread.
    let shared: SharedSnapshot = Arc::new(RwLock::new(Arc::new(Snapshot::empty(0))));

    // Remote/cloud hosts: fetched on their own cadence, cached, merged in.
    let remote_defs = remote::load_remotes(&paths.remotes_file());
    if !remote_defs.is_empty() {
        eprintln!("tracking {} remote host(s)", remote_defs.len());
    }
    let manager = RemoteManager::new(remote_defs);
    let remote_cache = manager.cache();
    let remote_defs = manager.defs();
    let remote_secs = std::env::var("CCWATCH_REMOTE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_REMOTE_SECS);
    manager.spawn(Duration::from_secs(remote_secs));

    // A tick channel drives refreshes: poll timer + file-change events both send.
    let (tick_tx, tick_rx) = mpsc::channel::<()>();
    spawn_poll_timer(tick_tx.clone());
    let _watcher = spawn_fs_watcher(&paths, tick_tx.clone());

    // Refresher thread owns the engine and merges cached remote snapshots.
    {
        let shared = shared.clone();
        let config = Config::load(&paths.config_file());
        let paths2 = paths.clone();
        let remote_cache = remote_cache.clone();
        let build = move |engine: &mut Engine| {
            let local = engine.refresh_now();
            let remotes = remote_cache.read().unwrap().clone();
            remote::merge(local, &remotes)
        };
        std::thread::spawn(move || {
            let mut engine = Engine::new(paths2, config);
            // Prime immediately so early subscribers get real data.
            *shared.write().unwrap() = Arc::new(build(&mut engine));
            for _ in tick_rx {
                *shared.write().unwrap() = Arc::new(build(&mut engine));
            }
        });
    }

    // Accept connections.
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let shared = shared.clone();
                let paths = paths.clone();
                let tick_tx = tick_tx.clone();
                let remote_defs = remote_defs.clone();
                std::thread::spawn(move || {
                    if let Err(e) = handle_client(stream, shared, paths, tick_tx, remote_defs) {
                        eprintln!("client error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
    Ok(())
}

fn spawn_poll_timer(tx: Sender<()>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(POLL_INTERVAL);
        if tx.send(()).is_err() {
            break;
        }
    });
}

/// Watch the Claude data dirs; coalesced events just send a tick.
fn spawn_fs_watcher(paths: &Paths, tx: Sender<()>) -> Option<notify::RecommendedWatcher> {
    use notify::{RecursiveMode, Watcher};
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok() {
            let _ = tx.send(());
        }
    })
    .ok()?;
    // Best-effort: watch each dir that exists.
    for dir in [paths.sessions(), paths.tasks(), paths.projects()] {
        let _ = watcher.watch(&dir, RecursiveMode::Recursive);
    }
    Some(watcher)
}

fn handle_client(
    stream: UnixStream,
    shared: SharedSnapshot,
    paths: Paths,
    tick_tx: Sender<()>,
    remote_defs: Arc<Vec<RemoteDef>>,
) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;

    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(()); // client hung up
    }
    let msg: ClientMsg = match serde_json::from_str(line.trim()) {
        Ok(m) => m,
        Err(e) => {
            send(&mut writer, &ServerMsg::ActionResult {
                ok: false,
                message: format!("bad request: {e}"),
            })?;
            return Ok(());
        }
    };

    match msg {
        ClientMsg::Subscribe => push_loop(&mut writer, &shared)?,
        ClientMsg::Action(req) => {
            let outcome = execute_action(&req, &paths, &remote_defs);
            let (ok, message) = match outcome {
                ActionOutcome::Ok(m) => (true, m),
                ActionOutcome::Failed(m) => (false, m),
            };
            log_action(&paths, &req, ok, &message);
            send(&mut writer, &ServerMsg::ActionResult { ok, message })?;
            let _ = tick_tx.send(()); // reflect the change quickly
        }
    }
    Ok(())
}

/// Push snapshots whenever they change, until the client disconnects (detected
/// on write error). A heartbeat goes out only after ~3s of silence, as a
/// liveness signal — not on every poll.
fn push_loop(writer: &mut UnixStream, shared: &SharedSnapshot) -> anyhow::Result<()> {
    let mut last_sent = 0i64;
    let mut last_write = std::time::Instant::now();
    loop {
        let snap = shared.read().unwrap().clone();
        if snap.generated_at != last_sent {
            last_sent = snap.generated_at;
            if send(writer, &ServerMsg::Snapshot(Box::new((*snap).clone()))).is_err() {
                break;
            }
            last_write = std::time::Instant::now();
        } else if last_write.elapsed() > Duration::from_secs(3) {
            if send(
                writer,
                &ServerMsg::Heartbeat {
                    at_ms: chrono::Utc::now().timestamp_millis(),
                },
            )
            .is_err()
            {
                break;
            }
            last_write = std::time::Instant::now();
        }
        std::thread::sleep(PUSH_INTERVAL);
    }
    Ok(())
}

fn execute_action(
    req: &ActionRequest,
    _paths: &Paths,
    remote_defs: &[RemoteDef],
) -> ActionOutcome {
    match req {
        ActionRequest::KillSession { pid } => actions::terminate_session(*pid, KILL_GRACE),
        ActionRequest::PauseSession { pid } => actions::pause(*pid),
        ActionRequest::ResumeSession { pid } => actions::resume(*pid),
        ActionRequest::KillBackground { pid } => actions::kill_background(*pid),
        ActionRequest::DisableHook {
            settings_path,
            event,
            command,
        } => actions::disable_hook(std::path::Path::new(settings_path), event, command),
        ActionRequest::CancelRemote { remote, id } => cancel_remote(remote_defs, remote, id),
    }
}

/// Run a remote host's configured cancel command for `id`.
fn cancel_remote(defs: &[RemoteDef], remote: &str, id: &str) -> ActionOutcome {
    let Some(def) = defs.iter().find(|d| d.name == remote) else {
        return ActionOutcome::Failed(format!("unknown remote '{remote}'"));
    };
    let Some(argv) = def.cancel_argv(id) else {
        return ActionOutcome::Failed(format!("remote '{remote}' has no cancel command"));
    };
    use ccwatch_core::remote::CommandRunner;
    match SystemRunner.run(&argv, KILL_GRACE.saturating_mul(5)) {
        Ok(out) => ActionOutcome::Ok(format!("cancelled {id} on {remote}: {}", out.trim())),
        Err(e) => ActionOutcome::Failed(format!("cancel {id} on {remote} failed: {e}")),
    }
}

fn send(writer: &mut UnixStream, msg: &ServerMsg) -> std::io::Result<()> {
    let mut line = serde_json::to_string(msg).unwrap_or_else(|_| "{}".into());
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    writer.flush()
}

fn read_pidfile(paths: &Paths) -> Option<i32> {
    std::fs::read_to_string(paths.pidfile())
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn log_action(paths: &Paths, req: &ActionRequest, ok: bool, message: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.action_log())
    {
        let ts = chrono::Utc::now().to_rfc3339();
        let _ = writeln!(
            f,
            "{ts}\t{}\tok={ok}\t{message}",
            serde_json::to_string(req).unwrap_or_default()
        );
    }
}
