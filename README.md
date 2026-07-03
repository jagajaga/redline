<p align="center"><img src="assets/icon.svg" width="140" alt="Redline"></p>

<h1 align="center">Redline</h1>

<p align="center">
Mission control for Claude Code.<br/>
Every session, every agent, every token — every machine.
</p>

<p align="center">
<a href="https://github.com/jagajaga/redline/actions/workflows/ci.yml"><img src="https://github.com/jagajaga/redline/actions/workflows/ci.yml/badge.svg" alt="ci"></a>
<a href="https://github.com/jagajaga/redline/releases"><img src="https://img.shields.io/badge/macOS-universal-black" alt="macos"></a>
</p>

---

Claude Code will happily burn your entire 5-hour window while you're at lunch.
Sessions pile up, agents spawn agents, a server grinds all night — and the
first you hear is **"limit reached, resets at 03:00."**

Redline already knows. It was watching.

![Redline menu bar](docs/screenshot-menubar.svg)

![Redline dashboard window](docs/screenshot-gui.svg)

![Redline terminal UI](docs/screenshot-tui.svg)

## Features

- ⛽ **The Governor** — a fuel gauge that **learns your real plan limits from
  your own 429s** (both the 5-hour window *and* the weekly cap), re-measuring on
  every confirmed wall so upgrades *and downgrades* self-correct. It shows the
  **binding** limit — whichever wall you'll hit first. `▲2.1×` = hitting it 40 min
  before reset; `▼0.6×` = coast home. Usage is weighted per model so the gauge
  stays honest across a Fable/Opus/Sonnet mix. Zero config.
- 🪟 **Redline, the app** — one native **SwiftUI** macOS app (menu bar + window,
  no Dock icon). The menu-bar icon is a **live burn-rate graph**; click it for a
  popover with the Governor and a live, per-session list — model, tokens,
  cpu/ram, current activity, agent counts, and **kill/pause/resume**. The
  **vibrancy dashboard window** shows the Governor as a ring, both limits side
  by side, the model mix, and a **click-to-expand, scrollable tree** of every
  session → agent → task → process. Opens the window by default; flip **Start
  with menu bar only** to launch tray-only.
- 🖥 **Fleet view** — all sessions, all machines: burn rate, tokens, cpu/ram,
  last activity, the same titles Claude's UI uses
- 🔍 **Live activity** — `✎ Edit engine.rs` · `⚙ cargo build 87%` — in-flight
  tool calls *and* real child processes, per session
- 🤖 **Agents** — who spawned what, nested, with truthful running/done state
  (background agents included)
- 🚨 **Leak alerts** — runaway loops, cache bleed, agent storms, sessions
  burning while "idle", servers gone dark
- 🔪 **Kill switch** — kill / pause / resume from menu bar or terminal,
  always with confirmation
- 🛰 **Remote machines, zero install** — one line of JSON; a python probe
  goes over ssh, nothing gets installed

## Install

**Redline (the macOS app)** — a native SwiftUI app (menu bar + window),
macOS 14+. Grab `Redline-*.zip` from the
[latest release](https://github.com/jagajaga/redline/releases/latest), unzip,
drop `Redline.app` into `/Applications`, open it. It opens the **dashboard
window** by default and lives in the **menu bar** — one app, both surfaces.
Flip **Start with menu bar only** (in the window's footer, or the tray's
Settings) to launch tray-only. The binary is unsigned, so the first launch
needs one of:

```sh
xattr -dr com.apple.quarantine /Applications/Redline.app
```

(or right-click → Open → Open).

**Terminal UI** — grab `ccwatch-*-macos-universal.tar.gz` from the same
release:

```sh
tar xzf ccwatch-*-macos-universal.tar.gz
mv ccwatch/ccwatch ccwatch/ccwatchd /usr/local/bin/   # or anywhere on PATH
ccwatch
```

**From source** (any of it):

```sh
cargo build --release && ./target/release/ccwatch   # daemon + TUI
swift build -c release --package-path app            # the macOS app
```

No setup. No accounts. No telemetry. The daemon starts itself and exits
itself when the last window closes.

## Drive

| | | | |
|---|---|---|---|
| `/` jump anywhere | `d` details | `s` sort | `enter` expand |
| `k` kill | `p` `r` pause/resume | `f` hide idle | `x` hide done |

Menu-bar icon → a **popover** (its list shows only actively-working sessions).
The **⚙ Settings** panel has two independent controls: **Limit** — which cap the
number reflects (**5h window / Weekly / Mix**, where Mix is whichever wall binds
first) — and **Menu bar shows** — the format next to the graph (**throttle ▲▼×,
percent, burn rate, or nothing**). Plus **Start with menu bar only** and **Start
at login**. The footer has **TUI** (opens the terminal UI) and **Dashboard**
(opens the window).

In the **dashboard window**: click any session to expand its agents, tasks,
activity, and child processes (click an agent to go deeper); toggle **Hide
inactive** / **Hide done** in the footer; scroll, resize, or close it like any
Mac window.

## Remote machines

```json
// ~/.claude/ccwatch/remotes.json
[{ "name": "my-server", "kind": "ssh", "target": "user@host" }]
```

Needs ssh keys and python3 on the box. That's the whole setup. Remote
sessions land in the same views, killable over ssh, and a dead server
becomes an alert — not a silent gap.

## Tune

Budgets are in **Opus-equivalent tokens** — usage is weighted by model
(Opus ×1, Fable ×2, Sonnet ×0.6, Haiku ×0.2) so a tank stays honest as the
model mix shifts. Leave a budget unset and Redline learns it from the wall.

```toml
# ~/.claude/ccwatch/config.toml — all optional
#window_budget = 200_000_000  # 5h plan window; unset → learned from 429s
#week_budget = 600_000_000    # weekly cap; unset → learned from limit markers
#weight_fable = 2.0           # override a model weight if pricing shifts
terminal = "iTerm"            # for "Open TUI dashboard"
burn_tokens_per_min = 40000   # where the graph turns red
```

## Under the hood

```
~/.claude  ──┐                      ┌── ssh ── remote ~/.claude
             ▼                      ▼
          ccwatchd ── tails transcripts (new bytes only), watches pids,
             │        computes rates · alerts · the Governor
             ├────────────────┐   unix socket, JSON snapshots
             ▼                ▼
        ccwatch (TUI)   redline (menu-bar tray + dashboard window)
```

Claude Code already writes everything worth knowing into `~/.claude`.
Redline just reads it well. Idle cost: ~0% CPU.
