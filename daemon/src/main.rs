//! `ccwatchd` — the always-on collector daemon.
//!
//! It owns the single [`Engine`], refreshes it on file-change events (with a
//! poll backstop), and serves newline-delimited JSON over a Unix domain socket:
//! subscribers get pushed snapshots; action requests are executed and answered.
//!
//! `ccwatchd --once` prints a single snapshot as JSON and exits — handy for
//! scripting and verification.

use ccwatch_core::actions::{self, ActionOutcome};
use ccwatch_core::ipc::{ActionRequest, ClientMsg, ServerMsg};
use ccwatch_core::model::Snapshot;
use ccwatch_core::{Config, Engine, Paths};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, RwLock};
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(1500);
const PUSH_INTERVAL: Duration = Duration::from_millis(500);
const KILL_GRACE: Duration = Duration::from_secs(2);

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

    // A tick channel drives refreshes: poll timer + file-change events both send.
    let (tick_tx, tick_rx) = mpsc::channel::<()>();
    spawn_poll_timer(tick_tx.clone());
    let _watcher = spawn_fs_watcher(&paths, tick_tx.clone());

    // Refresher thread owns the engine.
    {
        let shared = shared.clone();
        let config = Config::load(&paths.config_file());
        let paths2 = paths.clone();
        std::thread::spawn(move || {
            let mut engine = Engine::new(paths2, config);
            // Prime immediately so early subscribers get real data.
            *shared.write().unwrap() = Arc::new(engine.refresh_now());
            for _ in tick_rx {
                let snap = engine.refresh_now();
                *shared.write().unwrap() = Arc::new(snap);
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
                std::thread::spawn(move || {
                    if let Err(e) = handle_client(stream, shared, paths, tick_tx) {
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
            let outcome = execute_action(&req, &paths);
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

/// Push snapshots whenever they change, heartbeats otherwise, until the client
/// disconnects (detected on write error).
fn push_loop(writer: &mut UnixStream, shared: &SharedSnapshot) -> anyhow::Result<()> {
    let mut last_sent = 0i64;
    loop {
        let snap = shared.read().unwrap().clone();
        if snap.generated_at != last_sent {
            last_sent = snap.generated_at;
            if send(writer, &ServerMsg::Snapshot(Box::new((*snap).clone()))).is_err() {
                break;
            }
        } else if send(
            writer,
            &ServerMsg::Heartbeat {
                at_ms: chrono::Utc::now().timestamp_millis(),
            },
        )
        .is_err()
        {
            break;
        }
        std::thread::sleep(PUSH_INTERVAL);
    }
    Ok(())
}

fn execute_action(req: &ActionRequest, _paths: &Paths) -> ActionOutcome {
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
