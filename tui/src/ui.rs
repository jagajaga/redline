//! Rendering. A pure function of [`App`] → frame; no state mutation here.

use crate::app::{agent_at, App, Mode, RowRef};
use crate::format;
use ccwatch_core::model::{AgentState, Alert, Session, SessionState, Severity, WatcherKind};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

/// tok/min at which the burn column turns red (matches the daemon's default
/// `burn_tokens_per_min`; purely cosmetic here).
const BURN_RED: f64 = 40_000.0;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let alert_h = (app.snapshot.alerts.len() as u16).clamp(1, 4) + 2;
    let chunks = Layout::vertical([
        Constraint::Length(1),        // top bar
        Constraint::Length(alert_h),  // alerts
        Constraint::Min(5),           // tree
        Constraint::Length(8),        // tasks | watchers
        Constraint::Length(6),        // details
        Constraint::Length(1),        // footer
    ])
    .split(area);

    draw_topbar(f, chunks[0], app);
    draw_alerts(f, chunks[1], app);
    draw_tree(f, chunks[2], app);
    draw_bottom_split(f, chunks[3], app);
    draw_details(f, chunks[4], app);
    draw_footer(f, chunks[5], app);

    match &app.mode {
        Mode::Fuzzy { query, results, cursor } => draw_fuzzy(f, area, query, results, *cursor),
        Mode::Confirm(p) => draw_confirm(f, area, &p.prompt),
        Mode::Normal => {}
    }
}

fn draw_topbar(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.snapshot.totals;
    let conn = if app.connected { "●" } else { "○ disconnected" };
    let text = format!(
        " ccwatch  {conn}  local · {} active · {} tok/min · Σ {} · cache {:.0}%",
        t.active_sessions,
        format::rate(t.tokens_per_min),
        format::tokens(t.total_tokens),
        t.cache_hit_pct,
    );
    let style = if app.connected {
        Style::default().fg(Color::Black).bg(Color::Cyan)
    } else {
        Style::default().fg(Color::White).bg(Color::Red)
    };
    f.render_widget(
        Paragraph::new(text).style(style.add_modifier(Modifier::BOLD)),
        area,
    );
}

fn draw_alerts(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" ALERTS ({}) ", app.snapshot.alerts.len()));
    if app.snapshot.alerts.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "no leaks detected",
            Style::default().fg(Color::DarkGray),
        )))
        .block(block);
        f.render_widget(p, area);
        return;
    }
    let items: Vec<ListItem> = app
        .snapshot
        .alerts
        .iter()
        .map(|a| ListItem::new(alert_line(a)))
        .collect();
    f.render_widget(List::new(items).block(block), area);
}

fn alert_line(a: &Alert) -> Line<'static> {
    let color = match a.severity {
        Severity::Critical => Color::Red,
        Severity::Warn => Color::Yellow,
    };
    Line::from(vec![
        Span::styled("⚠ ", Style::default().fg(color)),
        Span::styled(
            format!("{:<14}", a.kind.label()),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{:<22}", a.subject), Style::default().fg(Color::White)),
        Span::raw(a.message.clone()),
    ])
}

fn draw_tree(f: &mut Frame, area: Rect, app: &App) {
    let rows = app.visible_rows();
    let block = Block::default().borders(Borders::ALL).title(Line::from(vec![
        Span::raw(" SESSIONS / AGENTS "),
        Span::styled(
            "  name/desc            state   up      tok/min  in/out/cw/cr        cpu   rss ",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    let inner_h = area.height.saturating_sub(2) as usize;
    let start = if app.selected >= inner_h {
        app.selected - inner_h + 1
    } else {
        0
    };

    let mut items = Vec::new();
    for (i, vr) in rows.iter().enumerate().skip(start).take(inner_h) {
        let selected = i == app.selected;
        let line = match &vr.row {
            RowRef::Session(si) => session_line(&app.snapshot.sessions[*si], app),
            RowRef::Agent(si, path) => {
                let a = agent_at(&app.snapshot.sessions[*si].agents, path);
                agent_line(a, vr.depth, app)
            }
        };
        let item = if selected {
            ListItem::new(line).style(Style::default().add_modifier(Modifier::REVERSED))
        } else {
            ListItem::new(line)
        };
        items.push(item);
    }
    if items.is_empty() {
        items.push(ListItem::new(Span::styled(
            "no active sessions",
            Style::default().fg(Color::DarkGray),
        )));
    }
    f.render_widget(List::new(items).block(block), area);
}

fn state_span(state: SessionState) -> Span<'static> {
    let (txt, color) = match state {
        SessionState::Running => ("running", Color::Green),
        SessionState::Idle => ("idle", Color::DarkGray),
        SessionState::Ended => ("ended", Color::DarkGray),
    };
    Span::styled(format!("{txt:<8}"), Style::default().fg(color))
}

fn burn_span(tpm: f64, threshold: f64) -> Span<'static> {
    let color = if tpm >= threshold {
        Color::Red
    } else if tpm >= threshold / 2.0 {
        Color::Yellow
    } else {
        Color::Gray
    };
    Span::styled(format!("{:>7}", format::rate(tpm)), Style::default().fg(color))
}

fn session_line(s: &Session, app: &App) -> Line<'static> {
    let up = s
        .started_at
        .map(|t| format::ago(t, app.now_ms))
        .unwrap_or_else(|| "-".into());
    let model = s
        .model
        .as_deref()
        .unwrap_or("-")
        .trim_start_matches("claude-")
        .to_string();
    let toks = &s.tokens;
    let breakdown = format!(
        "{}/{}/{}/{}",
        format::tokens(toks.input),
        format::tokens(toks.output),
        format::tokens(toks.cache_write),
        format::tokens(toks.cache_read),
    );
    let expand = if s.agents.is_empty() {
        "  "
    } else if app.expanded.contains(&s.id) {
        "▾ "
    } else {
        "▸ "
    };
    Line::from(vec![
        Span::raw(expand),
        Span::styled(
            format!("{:<20}", truncate(&s.name, 20)),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        state_span(s.state),
        Span::raw(format!("{up:<8}")),
        burn_span(s.tokens_per_min, BURN_RED),
        Span::raw("  "),
        Span::styled(format!("{breakdown:<18}"), Style::default().fg(Color::DarkGray)),
        Span::raw(format!(" {:>3.0}% ", s.cpu_pct)),
        Span::raw(format!("{:>4}M", s.rss_mb)),
        Span::styled(format!("  [{model}]"), Style::default().fg(Color::DarkGray)),
    ])
}

fn agent_line(a: Option<&ccwatch_core::model::Agent>, depth: usize, app: &App) -> Line<'static> {
    let Some(a) = a else {
        return Line::from("· <agent>");
    };
    let indent = "  ".repeat(depth);
    let branch = if a.children.is_empty() {
        "└ "
    } else if app.expanded.contains(&a.id) {
        "▾ "
    } else {
        "▸ "
    };
    let (st, color) = match a.state {
        AgentState::Running => ("running", Color::Green),
        AgentState::Finished => ("done", Color::DarkGray),
    };
    let up = a
        .started_at
        .map(|t| format::ago(t, app.now_ms))
        .unwrap_or_else(|| "-".into());
    Line::from(vec![
        Span::styled(format!("{indent}{branch}"), Style::default().fg(Color::Blue)),
        Span::styled(
            format!("{} ", truncate(&a.description, 26)),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(format!("[{}] ", a.subagent_type), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{st:<8}"), Style::default().fg(color)),
        Span::styled(up, Style::default().fg(Color::DarkGray)),
    ])
}

fn draw_bottom_split(f: &mut Frame, area: Rect, app: &App) {
    let halves = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);
    let session = app.selected_session();

    // Tasks
    let (done, total) = session
        .map(|s| {
            (
                s.tasks.iter().filter(|t| t.status == "completed").count(),
                s.tasks.len(),
            )
        })
        .unwrap_or((0, 0));
    let tblock = Block::default()
        .borders(Borders::ALL)
        .title(format!(" TASKS ({done}/{total}) "));
    let titems: Vec<ListItem> = session
        .map(|s| s.tasks.iter().map(task_item).collect())
        .unwrap_or_default();
    f.render_widget(
        List::new(if titems.is_empty() {
            vec![ListItem::new(Span::styled("—", Style::default().fg(Color::DarkGray)))]
        } else {
            titems
        })
        .block(tblock),
        halves[0],
    );

    // Watchers
    let wblock = Block::default().borders(Borders::ALL).title(" WATCHERS ");
    let witems: Vec<ListItem> = session
        .map(|s| s.watchers.iter().map(|w| watcher_item(w, app.now_ms)).collect())
        .unwrap_or_default();
    f.render_widget(
        List::new(if witems.is_empty() {
            vec![ListItem::new(Span::styled("—", Style::default().fg(Color::DarkGray)))]
        } else {
            witems
        })
        .block(wblock),
        halves[1],
    );
}

fn task_item(t: &ccwatch_core::model::Task) -> ListItem<'static> {
    let (glyph, color) = match t.status.as_str() {
        "completed" => ("✓", Color::Green),
        "in_progress" => ("●", Color::Yellow),
        _ => ("○", Color::Gray),
    };
    let mut spans = vec![
        Span::styled(format!("{glyph} "), Style::default().fg(color)),
        Span::raw(truncate(&t.subject, 32)),
    ];
    if t.blocked {
        spans.push(Span::styled(" ⛌blocked", Style::default().fg(Color::Red)));
    }
    ListItem::new(Line::from(spans))
}

fn watcher_item(w: &ccwatch_core::model::Watcher, now: i64) -> ListItem<'static> {
    let kind = match w.kind {
        WatcherKind::Hook => "hook",
        WatcherKind::Loop => "loop",
        WatcherKind::Routine => "routine",
        WatcherKind::Background => "bg",
    };
    let mut tail = String::new();
    if let Some(nw) = w.next_wake {
        tail = format!("  next {}", format::duration_ms(nw - now));
    } else if w.fired_count > 0 {
        tail = format!("  fired {}", w.fired_count);
    }
    ListItem::new(Line::from(vec![
        Span::styled(format!("{kind:<8}"), Style::default().fg(Color::Magenta)),
        Span::raw(format!("{:<8}", w.schedule.clone().unwrap_or_default())),
        Span::raw(truncate(&w.detail, 22)),
        Span::styled(tail, Style::default().fg(Color::DarkGray)),
    ]))
}

fn draw_details(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title(" DETAILS ");
    let Some(vr) = app.selected_row() else {
        f.render_widget(Paragraph::new("").block(block), area);
        return;
    };
    let lines: Vec<Line> = match &vr.row {
        RowRef::Session(si) => session_details(&app.snapshot.sessions[*si], app),
        RowRef::Agent(si, path) => match agent_at(&app.snapshot.sessions[*si].agents, path) {
            Some(a) => agent_details(a, &app.snapshot.sessions[*si], app),
            None => vec![Line::from("agent gone")],
        },
    };
    f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: true }), area);
}

fn session_details(s: &Session, app: &App) -> Vec<Line<'static>> {
    let t = &s.tokens;
    vec![
        Line::from(vec![
            Span::styled("session ", Style::default().fg(Color::DarkGray)),
            Span::styled(s.name.clone(), Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("   pid {}   {}   {}", s.pid.unwrap_or(0), s.kind, s.entrypoint)),
        ]),
        Line::from(format!("cwd {}", s.cwd)),
        Line::from(vec![
            Span::styled("tokens  ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!(
                "in {} · out {} · cw {} · cr {}   msgs {}   {} tok/min",
                format::tokens(t.input),
                format::tokens(t.output),
                format::tokens(t.cache_write),
                format::tokens(t.cache_read),
                t.messages,
                format::rate(s.tokens_per_min),
            )),
        ]),
        Line::from(vec![
            Span::styled(
                "legend  in=input  out=output  cw=cache-write  cr=cache-read",
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(match s.last_activity {
                Some(la) => format!("   last activity {} ago", format::ago(la, app.now_ms)),
                None => String::new(),
            }),
        ]),
    ]
}

fn agent_details(a: &ccwatch_core::model::Agent, s: &Session, app: &App) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled("agent ", Style::default().fg(Color::DarkGray)),
            Span::styled(a.description.clone(), Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(format!(
            "type {}   model {}   parent {}",
            a.subagent_type,
            a.model.as_deref().unwrap_or("-"),
            s.name
        )),
        Line::from(match a.started_at {
            Some(t) => format!(
                "started {}   ({} ago)   children {}",
                format::clock(t),
                format::ago(t, app.now_ms),
                a.children.len()
            ),
            None => format!("children {}", a.children.len()),
        }),
        Line::from(Span::styled(
            "note: subagents can't be stopped individually — kill the owning session",
            Style::default().fg(Color::DarkGray),
        )),
    ]
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let hint = match &app.mode {
        Mode::Fuzzy { .. } => " type to filter · ↑↓ move · enter jump · esc cancel ",
        Mode::Confirm(_) => " y confirm · n/esc cancel ",
        Mode::Normal => {
            " / jump · ↑↓ move · enter expand · k kill · p pause · r resume · f filter-idle · q quit "
        }
    };
    let mut spans = vec![Span::styled(hint, Style::default().fg(Color::Black).bg(Color::Gray))];
    if let Some(st) = &app.status {
        spans.push(Span::styled(format!("  {st}"), Style::default().fg(Color::Yellow)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_fuzzy(f: &mut Frame, area: Rect, query: &str, results: &[crate::app::JumpItem], cursor: usize) {
    let popup = centered(70, 60, area);
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" jump (fuzzy) ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(inner);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("› ", Style::default().fg(Color::Cyan)),
            Span::raw(query.to_string()),
            Span::styled("▏", Style::default().add_modifier(Modifier::SLOW_BLINK)),
        ])),
        rows[0],
    );
    let h = rows[1].height as usize;
    let start = if cursor >= h { cursor - h + 1 } else { 0 };
    let items: Vec<ListItem> = results
        .iter()
        .enumerate()
        .skip(start)
        .take(h)
        .map(|(i, it)| {
            let line = Line::from(vec![
                Span::styled(format!("{:<8}", it.kind), Style::default().fg(Color::DarkGray)),
                Span::raw(it.label.clone()),
            ]);
            if i == cursor {
                ListItem::new(line).style(Style::default().add_modifier(Modifier::REVERSED))
            } else {
                ListItem::new(line)
            }
        })
        .collect();
    f.render_widget(List::new(items), rows[1]);
}

fn draw_confirm(f: &mut Frame, area: Rect, prompt: &str) {
    let popup = centered(60, 20, area);
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" confirm ")
        .border_style(Style::default().fg(Color::Red));
    f.render_widget(
        Paragraph::new(prompt.to_string())
            .block(block)
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center),
        popup,
    );
}

fn centered(pct_x: u16, pct_y: u16, area: Rect) -> Rect {
    let v = Layout::vertical([
        Constraint::Percentage((100 - pct_y) / 2),
        Constraint::Percentage(pct_y),
        Constraint::Percentage((100 - pct_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - pct_x) / 2),
        Constraint::Percentage(pct_x),
        Constraint::Percentage((100 - pct_x) / 2),
    ])
    .split(v[1])[1]
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(130, 40)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn app_with(snap: ccwatch_core::model::Snapshot) -> App {
        let mut a = App::new(10_000);
        a.connected = true;
        a.set_snapshot(snap);
        a
    }

    #[test]
    fn renders_full_layout() {
        let snap = snapshot(vec![session(
            "s1",
            "webapp",
            vec![agent("a1", "search dir", vec![])],
        )]);
        let app = app_with(snap);
        let s = render(&app);
        for needle in [
            "ccwatch", "active", "ALERTS", "runaway", "webapp", "running", "TASKS", "do the thing",
            "WATCHERS", "DETAILS", "cache-write",
        ] {
            assert!(s.contains(needle), "expected screen to contain {needle:?}\n{s}");
        }
    }

    #[test]
    fn renders_fuzzy_overlay() {
        let snap = snapshot(vec![session("s1", "webapp", vec![])]);
        let mut app = app_with(snap);
        app.open_fuzzy();
        app.fuzzy_input('b');
        let s = render(&app);
        assert!(s.contains("jump (fuzzy)"), "overlay missing:\n{s}");
    }

    #[test]
    fn renders_confirm_overlay() {
        let snap = snapshot(vec![session("s1", "webapp", vec![])]);
        let mut app = app_with(snap);
        app.stage_action(crate::app::ActionKind::Kill);
        let s = render(&app);
        assert!(s.contains("confirm"), "confirm overlay missing:\n{s}");
        assert!(s.contains("Kill session"), "prompt missing:\n{s}");
    }
}
