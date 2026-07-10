# Cruise Control — Autonomous (Step 4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Let the daemon hold the pace on its own — pausing the plan's Background targets and resuming them once back under budget — **only when the user has explicitly opted in** (`cruise_mode = "auto"`), foreground always exempt, every action logged and reversible with one key.

**Architecture:** A shared `CruiseRuntime { mode, paced: HashSet<i32>, log }` lives in the daemon. `SetCruiseMode` (new IPC) sets `mode` at runtime; it also seeds from `config.cruise_mode`. Each refresh, after the plan is computed, if `mode == "auto"` the daemon reconciles: pause plan pids not yet paced, resume paced pids no longer in the plan (recovery), via the proven `actions::pause`/`resume`. A pure `reconcile_paced` computes the diff (unit-tested). The paced count + last log line ride in the snapshot for the UI; `SetCruiseMode("off")` releases everything.

**Tech Stack:** Rust (`ccwatch-core`, `ccwatch-daemon`, `ccwatch-tui`), Swift (`app`).

## Global Constraints

- Rust edition 2021. Clippy-clean under `-D warnings` (whole workspace).
- **DEFAULT OFF.** Nothing is auto-paused unless `cruise_mode` is `"auto"` (config or `SetCruiseMode`). `"off"`/`"advisory"`/`"oneclick"` never auto-pause.
- Only ever pause pids from `PacingPlan.pause_pids()` (Step-1 guarantees these are Background, never foreground). Resume only pids Cruise itself paced (tracked in `paced`).
- Reuse `core::actions::pause`/`resume`; no new signalling path.
- Setting `mode` to anything other than `"auto"` (esp. `"off"`) must RESUME every pid Cruise paced (release), so turning it off never leaves a session stuck.
- `saw_429` is wired from recent rate-limit events so a 429 tightens the target (Step-1 `plan()` already divides target by `aimd_cut` on `saw_429`).

---

### Task 1: `reconcile_paced` — the pure pause/resume diff

**Files:**
- Modify: `core/src/pacer.rs`

**Interfaces:**
- Produces: `pub fn reconcile_paced(plan_pause: &[i32], paced: &std::collections::HashSet<i32>) -> (Vec<i32>, Vec<i32>)` returning `(to_pause, to_resume)`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `core/src/pacer.rs`:

```rust
    #[test]
    fn reconcile_pauses_new_and_resumes_recovered() {
        use std::collections::HashSet;
        // Currently pacing pids {2,3}. The plan now wants {3,4}.
        // → pause 4 (new), resume 2 (no longer in the plan), leave 3.
        let paced: HashSet<i32> = [2, 3].into_iter().collect();
        let (to_pause, to_resume) = reconcile_paced(&[3, 4], &paced);
        assert_eq!(to_pause, vec![4]);
        assert_eq!(to_resume, vec![2]);
    }

    #[test]
    fn reconcile_empty_plan_resumes_all_paced() {
        use std::collections::HashSet;
        let paced: HashSet<i32> = [7, 8].into_iter().collect();
        let (to_pause, mut to_resume) = reconcile_paced(&[], &paced);
        to_resume.sort();
        assert!(to_pause.is_empty());
        assert_eq!(to_resume, vec![7, 8]);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ccwatch-core pacer::tests::reconcile_pauses_new_and_resumes_recovered`
Expected: FAIL — `reconcile_paced` not found.

- [ ] **Step 3: Implement**

Add to `core/src/pacer.rs`:

```rust
use std::collections::HashSet;

/// Given the pids the current plan wants paused and the set Cruise has already
/// paused, return `(to_pause, to_resume)`: newly-named pids to pause, and
/// already-paced pids no longer in the plan to resume (recovery). Pure.
pub fn reconcile_paced(plan_pause: &[i32], paced: &HashSet<i32>) -> (Vec<i32>, Vec<i32>) {
    let want: HashSet<i32> = plan_pause.iter().copied().collect();
    let to_pause: Vec<i32> = plan_pause.iter().copied().filter(|p| !paced.contains(p)).collect();
    let to_resume: Vec<i32> = paced.iter().copied().filter(|p| !want.contains(p)).collect();
    (to_pause, to_resume)
}
```

- [ ] **Step 4: Run tests + clippy**

Run: `cargo test -p ccwatch-core pacer::tests::` (all pass), `cargo clippy -p ccwatch-core --all-targets -- -D warnings` (clean).

- [ ] **Step 5: Commit**

```bash
git add core/src/pacer.rs
git commit -m "feat(pacer): reconcile_paced — pause new / resume recovered targets"
```

---

### Task 2: Daemon — autonomous apply + `SetCruiseMode` + log + release

**Files:**
- Modify: `core/src/ipc.rs` (add `ActionRequest::SetCruiseMode { mode: String }`)
- Modify: `core/src/model.rs` (add `PacingPlan.auto: bool` + `PacingPlan.paced: usize` so the UI can show autonomous state; `#[serde(default)]`)
- Modify: `daemon/src/main.rs` (shared `CruiseRuntime`; `SetCruiseMode` handler; auto-apply in the refresher; `saw_429`)

**Interfaces:**
- Consumes: `PacingPlan::pause_pids()`, `reconcile_paced`, `actions::pause`/`resume`, `config.cruise_mode`, the refresher's `snap.pacing`, the `rate_limits` the governor already tracks.
- Produces: `ActionRequest::SetCruiseMode { mode }`; `PacingPlan { …, auto: bool, paced: usize }`; daemon auto-pauses only in `"auto"`.

- [ ] **Step 1: Add the IPC variant + snapshot fields**

In `core/src/ipc.rs`, add to `ActionRequest`:

```rust
    /// Set Cruise Control mode at runtime: "off" | "advisory" | "oneclick" | "auto".
    /// Any non-"auto" value releases everything Cruise paused.
    SetCruiseMode { mode: String },
```

In `core/src/model.rs`, add to `struct PacingPlan` (with `#[serde(default)]`):

```rust
    /// True when the daemon is autonomously enforcing this plan (mode "auto").
    #[serde(default)]
    pub auto: bool,
    /// How many sessions Cruise currently has paused (autonomous mode).
    #[serde(default)]
    pub paced: usize,
```

Fix every `PacingPlan { .. }` literal the compiler flags (Step-1 `plan()` in `pacer.rs`, and any test) by adding `auto: false, paced: 0,`. In `pacer::plan`, set them explicitly (`auto: false` — the daemon overrides `auto`/`paced` when it enforces; keep `plan()` pure and unaware of runtime).

- [ ] **Step 2: Write the failing test (SetCruiseMode → mode + reconcile behavior)**

Add a daemon unit test (in `daemon/src/main.rs`'s test module) that exercises the pure reconcile wiring the daemon relies on — the daemon's own apply loop is I/O (signals), so test the decision, not the signalling:

```rust
#[cfg(test)]
mod cruise_tests {
    use ccwatch_core::pacer::reconcile_paced;
    use std::collections::HashSet;

    #[test]
    fn off_mode_releases_all_by_resuming_every_paced_pid() {
        // Turning cruise off (or any non-auto mode) is modeled as "plan wants
        // nothing paused" → reconcile resumes every paced pid.
        let paced: HashSet<i32> = [11, 12, 13].into_iter().collect();
        let (to_pause, to_resume) = reconcile_paced(&[], &paced);
        assert!(to_pause.is_empty());
        assert_eq!(to_resume.len(), 3);
    }
}
```

- [ ] **Step 3: Run test to verify it passes** (it uses only Task-1 code)

Run: `cargo test -p ccwatch-daemon cruise_tests`
Expected: PASS (this pins the release-on-off contract the daemon implements below).

- [ ] **Step 4: Add `CruiseRuntime` + `SetCruiseMode` handler**

In `daemon/src/main.rs`, define near the other shared state:

```rust
#[derive(Default)]
struct CruiseRuntime {
    mode: String,                 // "off" | "advisory" | "oneclick" | "auto"
    paced: std::collections::HashSet<i32>,
    last_log: Option<String>,
}
```

Create it as shared state seeded from config: `let cruise = std::sync::Arc::new(std::sync::Mutex::new(CruiseRuntime { mode: config.cruise_mode.clone(), ..Default::default() }));` — placed where other shared handles (e.g. `shared`, `subscribers`) are created, and cloned into both the refresher thread and `handle_client`/`execute_action`.

In `execute_action` (it will need the `cruise` handle — thread it through the same way `remote_defs` is passed), add:

```rust
        ActionRequest::SetCruiseMode { mode } => {
            let mut c = cruise.lock().unwrap();
            c.mode = mode.clone();
            let mut released = 0usize;
            if mode != "auto" {
                // Release: resume everything Cruise paused.
                for pid in c.paced.drain().collect::<Vec<_>>() {
                    if matches!(ccwatch_core::actions::resume(pid), ccwatch_core::actions::ActionOutcome::Ok(_)) {
                        released += 1;
                    }
                }
            }
            ActionOutcome::Ok(format!("Cruise mode = {mode}; released {released}"))
        }
```

(If `execute_action` is a free function without access to `cruise`, add a `cruise: &std::sync::Arc<std::sync::Mutex<CruiseRuntime>>` parameter and pass it at the call site — mirror how `remote_defs`/`snapshot` are already passed in.)

- [ ] **Step 5: Auto-apply in the refresher**

In the refresher closure, right after `snap.pacing = Some(plan);` (Step-1 wiring), add — reading the shared `cruise` (clone the `Arc` into the closure) and wiring `saw_429` from the rate limits the governor already has in scope (a fresh 429 within the last ~2 min):

```rust
            // Autonomous enforcement — ONLY in "auto". Otherwise leave the plan
            // advisory and make sure nothing stays paced.
            {
                let mut c = cruise.lock().unwrap();
                let plan_pause = snap.pacing.as_ref().map(|p| p.pause_pids()).unwrap_or_default();
                let auto = c.mode == "auto";
                let target_pause: Vec<i32> = if auto { plan_pause } else { Vec::new() };
                let (to_pause, to_resume) =
                    ccwatch_core::pacer::reconcile_paced(&target_pause, &c.paced);
                for pid in to_resume {
                    let _ = ccwatch_core::actions::resume(pid);
                    c.paced.remove(&pid);
                }
                for pid in to_pause {
                    if matches!(ccwatch_core::actions::pause(pid), ccwatch_core::actions::ActionOutcome::Ok(_)) {
                        c.paced.insert(pid);
                    }
                }
                if let Some(p) = snap.pacing.as_mut() {
                    p.auto = auto;
                    p.paced = c.paced.len();
                }
            }
```

(For `saw_429`: in the Step-1 `plan(...)` call earlier in the closure, replace the hard-coded `false` with a check like `snap.rate_limits.iter().any(|t| snap.generated_at - t < 120_000)` — use the field the engine/governor already populates for 429 timestamps; grep `rate_limit` in the closure to find it. If none is in scope, leave `false` and note it.)

- [ ] **Step 6: Build + tests + clippy**

Run: `cargo build --workspace` (fix any `PacingPlan` literal / match sites), `cargo test --workspace` (all pass), `cargo clippy --workspace --all-targets -- -D warnings` (clean).

- [ ] **Step 7: Smoke test (default OFF must not pause anything)**

Run the daemon against an isolated config with NO `cruise_mode` (defaults to "off") and confirm via a socket subscribe that `pacing.auto == false` and nothing gets paused. Document the check in the report. (Do NOT test "auto" against the user's real sessions — that would pause their live work; if you validate "auto", do it against a throwaway sleep-process pid in an isolated `CLAUDE_CONFIG_DIR`.)

- [ ] **Step 8: Commit**

```bash
git add core/src/ipc.rs core/src/model.rs daemon/src/main.rs
git commit -m "feat(cruise): autonomous enforcement behind opt-in mode (default off)"
```

---

### Task 3: UI — mode control + paced state + release

**Files:**
- Modify: `app/Sources/Redline/Store.swift` (`setCruiseMode(_:)`)
- Modify: `app/Sources/Redline/Menu.swift` (mode picker + "auto: N paused" + Release)
- Modify: `tui/src/app.rs` + `tui/src/ui.rs` (a key to cycle mode / release; show auto state)

**Interfaces:**
- Consumes: `snap.pacing.auto`, `snap.pacing.paced`; `ActionRequest::SetCruiseMode`.
- Produces: `Store.setCruiseMode(_ mode: String)`.

- [ ] **Step 1: App store method**

In `app/Sources/Redline/Store.swift`:

```swift
    nonisolated func setCruiseMode(_ mode: String) {
        client.sendAction(["msg": "action", "action": "set_cruise_mode", "mode": mode])
    }
```

- [ ] **Step 2: App — mode control in the settings panel**

In `app/Sources/Redline/Menu.swift`'s `settingsPanel`, add a Cruise row (mirror the existing `row("Limit") { Picker … }` idiom; persist the choice with `@AppStorage("cruise_mode")` and push it on change):

```swift
            row("Cruise") {
                Picker("", selection: $cruiseMode) {
                    Text("Off").tag("off")
                    Text("Advise").tag("advisory")
                    Text("Auto").tag("auto")
                }.pickerStyle(.segmented).labelsHidden()
                .onChange(of: cruiseMode) { _, m in store.setCruiseMode(m) }
            }
```

Add `@AppStorage("cruise_mode") private var cruiseMode = "off"` to `MenuContent`. In the Governor header advisory block, when `plan.auto`, change the label to show it's automatic and add a Release button:

```swift
                    if plan.auto {
                        Text("⚙︎ Cruise auto · \(plan.paced) paused")
                            .font(.caption2).foregroundStyle(Palette.teal)
                        Button("Release") { store.setCruiseMode("off"); cruiseMode = "off" }
                            .buttonStyle(.borderless).font(.caption2).foregroundStyle(Palette.teal)
                    }
```

- [ ] **Step 3: App build**

Run: `swift build --package-path app` → `Build complete!`.

- [ ] **Step 4: TUI — show auto state + a release key**

In `tui/src/ui.rs`, extend `pacing_advisory` (or add a small line) so that when `snap.pacing.auto` is true it reads `Cruise AUTO · N paused` instead of the recommendation. In `tui/src/app.rs`/`main.rs`, bind a key (e.g. Shift+`R` or `X`) that, only when `snap.pacing.auto`, stages a confirm sending `SetCruiseMode { mode: "off" }` (release). Follow the Step-3 `stage_apply_pacing` pattern; add a focused test asserting the pending action is `SetCruiseMode { mode: "off" }`.

- [ ] **Step 5: TUI tests + clippy**

Run: `cargo test -p ccwatch-tui` (all pass), `cargo clippy --workspace --all-targets -- -D warnings` (clean).

- [ ] **Step 6: Commit**

```bash
git add app/Sources/Redline/Store.swift app/Sources/Redline/Menu.swift tui/src/app.rs tui/src/ui.rs
git commit -m "feat(ui): Cruise mode control + autonomous paced state + release"
```

---

## Self-Review

**Spec coverage (Autonomous step):**
- Daemon control loop applies the plan in "auto" → Task 2 (`reconcile_paced` + refresher apply). ✓
- Per-tier opt-in, default off → `cruise_mode` gate; `SetCruiseMode`; Task 3 mode control. ✓
- Foreground exempt → guaranteed by `pause_pids()` (Step-1 invariant). ✓
- Resume-on-recovery + one-key release → `reconcile_paced` resumes recovered pids; `SetCruiseMode(non-auto)` releases all; Task 3 Release. ✓
- AIMD on 429 → `saw_429` wired to the plan's target-tightening. ✓
- Visible state → `pacing.auto`/`pacing.paced` in the snapshot, shown in both UIs. ✓

**Deferred (by design):** a full scrollable action-log view (only the paced count + last line surface for now); pausing strictly between agent turns (accepted: a long-paused turn may retry on resume — documented). The reservation/deadline live picker remains a config-only setting.

**Placeholder scan:** none — code or exact commands throughout. The "grep for the 429 field / thread the `cruise` handle like `remote_defs`" notes are integration-discovery steps with named fallbacks.

**Type consistency:** `reconcile_paced(&[i32], &HashSet<i32>) -> (Vec<i32>, Vec<i32>)`, `ActionRequest::SetCruiseMode { mode: String }` → `"set_cruise_mode"`, `PacingPlan { …, auto: bool, paced: usize }`, `Store.setCruiseMode(_:)` used consistently.
