//! `ccwatch-menubar` — Phase 3 macOS menu-bar client.
//!
//! It reuses the exact same daemon and IPC as the TUI: subscribe to snapshots,
//! show a glanceable title (`⚡3 · 46k` or `⚠2` when leaks are present), and a
//! dropdown listing per-host sessions. All presentation logic lives in
//! [`summary`] and is unit-tested; this file is the thin GUI shell.
//!
//! `ccwatch-menubar --dump` prints the title/tooltip/menu once and exits — a
//! headless way to verify the pipeline without a GUI session.

mod client;
mod graph;
mod summary;

use ccwatch_core::Paths;
use std::time::Duration;

/// Menu-bar graph icon size (points; macOS scales for Retina).
const GRAPH_W: usize = 48;
const GRAPH_H: usize = 18;

fn main() -> anyhow::Result<()> {
    let paths = Paths::discover();

    if std::env::args().any(|a| a == "--dump") {
        client::ensure_daemon(&paths)?;
        let snap = client::latest_snapshot(&paths, Duration::from_secs(5), Duration::from_secs(3))?;
        println!("title:   {}", summary::tray_title(&snap));
        println!("tooltip: {}", summary::tooltip(&snap));
        let rates: Vec<f64> = snap.sessions.iter().map(|s| s.tokens_per_min).collect();
        println!("load:    {}  (per-session tok/min)", graph::unicode_spark(&rates));
        println!("menu:");
        for line in summary::menu_lines(&snap) {
            println!("  {line}");
        }
        return Ok(());
    }

    run(&paths)
}

#[cfg(target_os = "macos")]
fn run(paths: &Paths) -> anyhow::Result<()> {
    use std::time::Instant;
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::TrayIconBuilder;

    client::ensure_daemon(paths)?;
    let rx = client::subscribe(paths)?;

    let event_loop = EventLoopBuilder::new().build();

    // A fixed pool of disabled rows we update by text each refresh, plus a
    // separator and Quit. Avoids add/remove churn on every snapshot.
    const ROWS: usize = 14;
    let menu = Menu::new();
    let rows: Vec<MenuItem> = (0..ROWS)
        .map(|_| MenuItem::new("", false, None))
        .collect();
    for r in &rows {
        let _ = menu.append(r);
    }
    let _ = menu.append(&PredefinedMenuItem::separator());
    let quit = MenuItem::new("Quit ccwatch", true, None);
    let _ = menu.append(&quit);

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_title("ccwatch …")
        .with_tooltip("ccwatch")
        .build()?;

    let menu_channel = MenuEvent::receiver();
    let quit_id = quit.id().clone();
    // Rolling history of total tok/min, one sample per graph column.
    let mut history = graph::History::new(GRAPH_W);

    event_loop.run(move |_event, _target, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(400));

        // Apply the latest snapshot (drain any backlog, keep the newest).
        let mut latest = None;
        while let Ok(snap) = rx.try_recv() {
            latest = Some(snap);
        }
        if let Some(snap) = latest {
            // Live load graph rendered as the menu-bar icon (iStat-style).
            history.push(snap.totals.tokens_per_min);
            let rgba = graph::render_rgba(&history.values(), GRAPH_W, GRAPH_H);
            if let Ok(icon) = tray_icon::Icon::from_rgba(rgba, GRAPH_W as u32, GRAPH_H as u32) {
                let _ = tray.set_icon(Some(icon));
            }
            // Keep a compact text readout beside the graph (alerts win).
            tray.set_title(Some(summary::tray_title(&snap)));
            let _ = tray.set_tooltip(Some(summary::tooltip(&snap)));
            let lines = summary::menu_lines(&snap);
            for (i, row) in rows.iter().enumerate() {
                row.set_text(lines.get(i).cloned().unwrap_or_default());
            }
        }

        if let Ok(ev) = menu_channel.try_recv() {
            if ev.id == quit_id {
                *control_flow = ControlFlow::Exit;
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn run(_paths: &Paths) -> anyhow::Result<()> {
    anyhow::bail!("the menu-bar client is macOS-only; use `--dump` or the TUI on this platform")
}
