# Cruise Control — One-Click Apply (Step 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Let the user apply the pacing recommendation with one action — pause exactly the Background sessions the plan already named — from the TUI and the app, reusing the proven `pause` actuator. User-triggered enforcement; nothing automatic.

**Architecture:** A new `ActionRequest::ApplyPacing` reaches the daemon, whose `execute_action` already holds the latest `snapshot` (with `pacing`). It pauses each pid in `snapshot.pacing.pause_pids()` via `core::actions::pause` (SIGSTOP — the same call behind the existing Pause button) and reports the count. The TUI binds a key to it (with a confirm); the app adds an Apply button on the advisory. This `ApplyPacing` is also the primitive Step 4 (Autonomous) will call on a loop.

**Tech Stack:** Rust (`ccwatch-core`, `ccwatch-daemon`, `ccwatch-tui`), Swift (`app`), existing IPC + `actions::pause`.

## Global Constraints

- Rust edition 2021. Clippy-clean under `-D warnings` (whole workspace).
- `ApplyPacing` pauses ONLY the pids the daemon's own latest plan named (`PacingPlan.pause_pids()`), which by construction excludes the foreground (Step 1 guarantees foreground is never in a Pause action). It must never pause anything not in that list.
- Reuse `core::actions::pause(pid) -> ActionOutcome`; do NOT add a new signalling path.
- `ActionRequest` serializes with the existing tag (`{"msg":"action","action":"<snake_case>"}`) — `ApplyPacing` → `"apply_pacing"`, no pid field.
- The app has no unit-test target — the Swift task is verified by `swift build --package-path app` + a review check that it sends `apply_pacing` and calls no other actuator directly.

---

### Task 1: `ApplyPacing` — pure pid list + IPC + daemon handler

**Files:**
- Modify: `core/src/model.rs` (add `PacingPlan::pause_pids`)
- Modify: `core/src/ipc.rs` (add `ActionRequest::ApplyPacing`)
- Modify: `daemon/src/main.rs` (add the `execute_action` arm)

**Interfaces:**
- Produces: `impl PacingPlan { pub fn pause_pids(&self) -> Vec<i32> }`; `ActionRequest::ApplyPacing`; daemon behavior "pause every plan-named pid, report count".

- [ ] **Step 1: Write the failing test (pure pid extraction)**

Add to the `#[cfg(test)] mod tests` in `core/src/model.rs` (create if absent — Step 1 added a `Priority`; there may already be a tests module):

```rust
    #[test]
    fn pacing_plan_pause_pids_lists_only_pause_targets() {
        let plan = PacingPlan {
            target_rate: 0.0,
            actual_rate: 0.0,
            price: 0.0,
            actions: vec![
                PaceAction::Pause { pid: 10, reason: "a".into() },
                PaceAction::Resume { pid: 20 },
                PaceAction::Pause { pid: 30, reason: "b".into() },
            ],
            reason: String::new(),
        };
        assert_eq!(plan.pause_pids(), vec![10, 30]);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ccwatch-core pacing_plan_pause_pids_lists_only_pause_targets`
Expected: FAIL — `pause_pids` not found.

- [ ] **Step 3: Implement `pause_pids`**

In `core/src/model.rs`, add:

```rust
impl PacingPlan {
    /// The pids the plan recommends pausing (Pause actions only). By construction
    /// (see `pacer::plan`) these are always Background sessions — never foreground.
    pub fn pause_pids(&self) -> Vec<i32> {
        self.actions
            .iter()
            .filter_map(|a| match a {
                PaceAction::Pause { pid, .. } => Some(*pid),
                PaceAction::Resume { .. } => None,
            })
            .collect()
    }
}
```

- [ ] **Step 4: Add the IPC variant**

In `core/src/ipc.rs`, add to `enum ActionRequest` (a unit variant — it carries no data; the daemon reads the plan from its own snapshot):

```rust
    /// Apply the daemon's current pacing plan once: pause every session it named.
    ApplyPacing,
```

- [ ] **Step 5: Add the daemon handler**

In `daemon/src/main.rs`, in `execute_action`'s `match req { ... }`, add an arm (uses the `snapshot` parameter it already receives, and `ccwatch_core::actions`):

```rust
        ActionRequest::ApplyPacing => {
            let pids = snapshot
                .pacing
                .as_ref()
                .map(|p| p.pause_pids())
                .unwrap_or_default();
            let mut paused = 0usize;
            for pid in pids {
                if matches!(ccwatch_core::actions::pause(pid), ccwatch_core::actions::ActionOutcome::Ok(_)) {
                    paused += 1;
                }
            }
            ActionOutcome::Ok(format!("Cruise: paused {paused} background session(s)"))
        }
```

(Match the exact `ActionOutcome`/`actions` import style already used by the neighboring arms — e.g. if they call `actions::pause(...)` with a `use` alias, use the same.)

- [ ] **Step 6: Run tests + build the workspace**

Run: `cargo test -p ccwatch-core pacing_plan_pause_pids_lists_only_pause_targets` (PASS), then `cargo build --workspace` (compiles — the new `ActionRequest` variant must be handled everywhere `ActionRequest` is matched exhaustively; fix any non-exhaustive-match errors the compiler flags, e.g. in `tui` or `remote`).

- [ ] **Step 7: Clippy + commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings` (clean), then:

```bash
git add core/src/model.rs core/src/ipc.rs daemon/src/main.rs
git commit -m "feat(cruise): ApplyPacing action — pause the plan's named sessions"
```

---

### Task 2: TUI — bind a key to apply pacing

**Files:**
- Modify: `tui/src/app.rs` (keybind → confirm → `ApplyPacing`)
- Modify: `tui/src/ui.rs` (footer hint; optional)

**Interfaces:**
- Consumes: the confirm flow (`Mode::Confirm(PendingAction { request, prompt })`, `take_pending`), `ActionRequest::ApplyPacing`, `app.snapshot.pacing`.

- [ ] **Step 1: Write the failing test**

Read `tui/src/app.rs` first to match the existing key-handling + `stage_*` pattern (see `stage_kill_enters_confirm_with_pid`). Add a test mirroring it:

```rust
    #[test]
    fn apply_cruise_key_stages_apply_pacing_confirm() {
        use ccwatch_core::model::{PaceAction, PacingPlan};
        let mut app = App::new(0);
        let mut snap = /* the same helper the other app tests use to build a Snapshot */;
        snap.pacing = Some(PacingPlan {
            target_rate: 100_000.0, actual_rate: 500_000.0, price: 1e-5,
            actions: vec![PaceAction::Pause { pid: 7, reason: "x".into() }],
            reason: "over".into(),
        });
        app.set_snapshot(snap);
        app.on_key(/* the KeyEvent for 'C' — match how other tests build KeyEvents */);
        match app.take_pending() {
            Some(ccwatch_core::ipc::ActionRequest::ApplyPacing) => {}
            other => panic!("expected ApplyPacing confirm, got {other:?}"),
        }
    }
```

Adapt the snapshot builder and `on_key`/KeyEvent construction to whatever the existing app tests use (grep the test module for how `stage_kill_enters_confirm_with_pid` builds its input — reuse it exactly).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ccwatch-tui app::tests::apply_cruise_key_stages_apply_pacing_confirm`
Expected: FAIL — the key isn't handled.

- [ ] **Step 3: Implement**

In `tui/src/app.rs`'s key handler (Normal mode), add a branch for `KeyCode::Char('C')` that, only when `self.snapshot.pacing.as_ref().is_some_and(|p| !p.pause_pids().is_empty())`, stages the confirm:

```rust
            KeyCode::Char('C') => {
                if let Some(p) = self.snapshot.pacing.as_ref() {
                    let n = p.pause_pids().len();
                    if n > 0 {
                        self.mode = Mode::Confirm(PendingAction {
                            request: ActionRequest::ApplyPacing,
                            prompt: format!("Pause {n} background session(s) to coast?"),
                        });
                    }
                }
            }
```

(Place it beside the existing `Char('k')`/`Char('p')` arms; match their exact structure. If `PacingPlan`/`pause_pids` need importing, add the `use`.)

- [ ] **Step 4: Footer hint (optional, same commit)**

If the Normal-mode footer hint string in `tui/src/ui.rs` lists keys, add `C cruise` next to `k kill`. Skip if it would overflow.

- [ ] **Step 5: Run tests + clippy**

Run: `cargo test -p ccwatch-tui` (all pass) and `cargo clippy -p ccwatch-tui --all-targets -- -D warnings` (clean).

- [ ] **Step 6: Commit**

```bash
git add tui/src/app.rs tui/src/ui.rs
git commit -m "feat(tui): press C to apply the Cruise pacing recommendation"
```

---

### Task 3: App — Apply button on the advisory

**Files:**
- Modify: `app/Sources/Redline/Store.swift` (add `applyPacing()`)
- Modify: `app/Sources/Redline/Menu.swift` (Apply button beside the advisory)

**Interfaces:**
- Consumes: `Store.sendAction(_:)`; `snap.pacing`.
- Produces: `Store.applyPacing()` sending `{"msg":"action","action":"apply_pacing"}`.

- [ ] **Step 1: Add the store method**

In `app/Sources/Redline/Store.swift`, beside `pauseSession`/`resumeSession`:

```swift
    nonisolated func applyPacing() {
        client.sendAction(["msg": "action", "action": "apply_pacing"])
    }
```

- [ ] **Step 2: Add the Apply button**

In `app/Sources/Redline/Menu.swift`, in the advisory block added in Step 2 (the `if let plan = snap.pacing, !plan.actions.isEmpty { ... }` inside `header(_ snap:)`), replace the single `Text("⏸ Cruise · \(first)")` with a row that keeps the text and adds an Apply button:

```swift
                if let plan = snap.pacing, !plan.actions.isEmpty,
                   let first = plan.actions.first(where: { $0.op == "pause" })?.reason {
                    HStack(spacing: 6) {
                        Text("⏸ Cruise · \(first)")
                            .font(.caption2).foregroundStyle(Palette.teal).lineLimit(2)
                        Spacer()
                        Button("Apply") { store.applyPacing() }
                            .buttonStyle(.borderless).font(.caption2).foregroundStyle(Palette.teal)
                    }
                }
```

- [ ] **Step 3: Build**

Run: `swift build --package-path app`
Expected: `Build complete!`

- [ ] **Step 4: Commit**

```bash
git add app/Sources/Redline/Store.swift app/Sources/Redline/Menu.swift
git commit -m "feat(app): Apply button to run the Cruise recommendation"
```

---

## Self-Review

**Spec coverage (One-click step):**
- Recommendations become an actionable one-click → Task 1 (`ApplyPacing`) + Task 2 (TUI key) + Task 3 (app button). ✓
- Only Background/plan-named sessions paused; foreground never → guaranteed by `pause_pids()` sourcing from the plan (Step 1 invariant); tested in Task 1. ✓
- Reuses the proven actuator → `core::actions::pause`. ✓

**Deferred (by design):** the live reservation/deadline *picker* and the mode segmented control (reservation already works via `config.toml`'s `cruise_reserve`; the mode gate lands with Step 4 Autonomous). A visible action-log lands with Step 4.

**Placeholder scan:** none — code or exact commands throughout. Task 2's "match the existing test's KeyEvent/snapshot builder" and Task 1 Step 6's "fix non-exhaustive matches" are verification steps, not placeholders.

**Type consistency:** `PacingPlan::pause_pids() -> Vec<i32>`, `ActionRequest::ApplyPacing`, `Store.applyPacing()` used consistently; action wire string `"apply_pacing"`.
