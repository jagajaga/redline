//! Integration test: spawn the real `ccwatchd`, subscribe over the socket, and
//! confirm it streams a snapshot that reflects a live session on disk. Also
//! exercises an action round-trip.

use ccwatch_core::ipc::{ActionRequest, ClientMsg, ServerMsg};
use ccwatch_core::Paths;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn connect(paths: &Paths) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if let Ok(s) = UnixStream::connect(paths.socket()) {
            return s;
        }
        assert!(Instant::now() < deadline, "daemon socket never came up");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn daemon_streams_snapshot_and_handles_action() {
    let root = std::env::temp_dir().join(format!("ccw-ipc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths::new(&root);

    // A live session: use our own pid so the daemon's probe sees it alive.
    let pid = std::process::id() as i32;
    let sid = "ipc-sess";
    std::fs::create_dir_all(paths.sessions()).unwrap();
    std::fs::write(
        paths.sessions().join(format!("{pid}.json")),
        format!(
            r#"{{"pid":{pid},"sessionId":"{sid}","cwd":"/tmp/ipc","kind":"interactive","name":"ipc-test","startedAt":{}}}"#,
            chrono::Utc::now().timestamp_millis() - 5000
        ),
    )
    .unwrap();
    let proj = paths.projects().join("-tmp-ipc");
    std::fs::create_dir_all(&proj).unwrap();
    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    std::fs::write(
        proj.join(format!("{sid}.jsonl")),
        format!(
            "{}\n",
            format_args!(
                r#"{{"type":"assistant","timestamp":"{ts}","message":{{"model":"claude-opus-4-8","usage":{{"input_tokens":10,"output_tokens":20}}}}}}"#
            )
        ),
    )
    .unwrap();

    // Spawn the daemon pointed at our fixture root.
    let _child = Child(
        std::process::Command::new(env!("CARGO_BIN_EXE_ccwatchd"))
            .env("CLAUDE_CONFIG_DIR", &root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn ccwatchd"),
    );

    // Subscribe and wait for a snapshot with our session.
    let stream = connect(&paths);
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    writer
        .write_all((serde_json::to_string(&ClientMsg::Subscribe).unwrap() + "\n").as_bytes())
        .unwrap();
    writer.flush().unwrap();

    let deadline = Instant::now() + Duration::from_secs(6);
    let mut found = false;
    let mut line = String::new();
    while Instant::now() < deadline {
        line.clear();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        if let Ok(ServerMsg::Snapshot(snap)) = serde_json::from_str::<ServerMsg>(line.trim()) {
            if let Some(s) = snap.sessions.iter().find(|s| s.id == sid) {
                assert_eq!(s.name, "ipc-test");
                assert_eq!(s.tokens.output, 20);
                found = true;
                break;
            }
        }
    }
    assert!(found, "never received a snapshot containing our session");

    // Action round-trip on a fresh connection: pause+resume a throwaway child.
    let mut victim = std::process::Command::new("sleep").arg("30").spawn().unwrap();
    let vpid = victim.id() as i32;
    let (ok, msg) = send_action(&paths, ActionRequest::PauseSession { pid: vpid });
    assert!(ok, "pause failed: {msg}");
    let (ok, msg) = send_action(&paths, ActionRequest::ResumeSession { pid: vpid });
    assert!(ok, "resume failed: {msg}");
    let (ok, _) = send_action(&paths, ActionRequest::KillBackground { pid: vpid });
    assert!(ok);
    let _ = victim.wait();

    // Action log was written.
    assert!(paths.action_log().exists(), "action log missing");

    let _ = std::fs::remove_dir_all(&root);
}

fn send_action(paths: &Paths, req: ActionRequest) -> (bool, String) {
    let mut stream = UnixStream::connect(paths.socket()).unwrap();
    stream
        .write_all((serde_json::to_string(&ClientMsg::Action(req)).unwrap() + "\n").as_bytes())
        .unwrap();
    stream.flush().unwrap();
    let mut reader = BufReader::new(stream);
    let mut resp = String::new();
    reader.read_line(&mut resp).unwrap();
    match serde_json::from_str::<ServerMsg>(resp.trim()).unwrap() {
        ServerMsg::ActionResult { ok, message } => (ok, message),
        _ => (false, "unexpected".into()),
    }
}
