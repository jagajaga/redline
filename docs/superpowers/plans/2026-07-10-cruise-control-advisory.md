# Cruise Control — Advisory Surfaces (Step 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Surface the pacing plan (already computed and attached to the snapshot in Step 1) as read-only advice in the TUI and the macOS app — the target pace and a concrete "pause these to coast" recommendation — with **no actions taken**.

**Architecture:** The daemon already puts `snap.pacing: Option<PacingPlan>` in every snapshot. The TUI (Rust, unit-testable via ratatui `TestBackend`) renders a one-line Cruise advisory in the ALERTS area. The SwiftUI app decodes the plan and shows the same advice in the menu-bar Governor header. Nothing calls an actuator.

**Tech Stack:** Rust (`ccwatch-tui`, ratatui), Swift (`app`, SwiftUI), existing `PacingPlan`/`PaceAction` model types.

## Global Constraints

- Rust edition 2021. Clippy-clean under `-D warnings` (`cargo clippy --workspace --all-targets -- -D warnings`).
- READ-ONLY: no surface may call pause/resume/kill from the pacing plan in this step.
- `PacingPlan` fields (from Step 1, `core/src/model.rs`): `target_rate: f64`, `actual_rate: f64`, `price: f64`, `actions: Vec<PaceAction>`, `reason: String`. `PaceAction` is `#[serde(tag = "op", rename_all = "snake_case")]` with `Pause { pid: i32, reason: String }` and `Resume { pid: i32 }`.
- The SwiftUI app has no unit-test target — Swift tasks are verified by `swift build --package-path app` plus a described manual check, not automated tests.
- Match existing rendering idioms: TUI advisory mirrors the existing `governor_estimated` "install extension" line in `draw_alerts`; the app line mirrors the existing `paceLine`/alert rows in `Menu.swift`'s Governor header.

---

### Task 1: TUI Cruise advisory line

Render a one-line Cruise recommendation in the ALERTS panel when the plan proposes pauses. TDD via `TestBackend`.

**Files:**
- Modify: `tui/src/ui.rs` (add `pacing_advisory`, wire into `draw` height + `draw_alerts`, add a test)

**Interfaces:**
- Consumes: `app.snapshot.pacing: Option<ccwatch_core::model::PacingPlan>`; `PacingPlan { actions, reason, target_rate, actual_rate }`; `PaceAction::Pause { reason, .. }`.
- Produces: `fn pacing_advisory(app: &App) -> Option<String>`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `tui/src/ui.rs` (uses the existing `snapshot`/`render`/`app_with` helpers — build a snapshot then attach a plan):

```rust
    #[test]
    fn renders_cruise_advisory_when_plan_proposes_pauses() {
        use ccwatch_core::model::{PaceAction, PacingPlan};
        let mut snap = snapshot(vec![session("s1", "webapp", vec![])]);
        snap.pacing = Some(PacingPlan {
            target_rate: 100_000.0,
            actual_rate: 500_000.0,
            price: 1.0e-5,
            actions: vec![PaceAction::Pause {
                pid: 42,
                reason: "pause fleet score_v3 (52 agents): 300000/min (value-density 1.0e-5)".into(),
            }],
            reason: "500000 over target → pausing 1 background session(s)".into(),
        });
        let app = app_with(snap);
        let s = render(&app);
        assert!(s.contains("Cruise"), "advisory label missing:\n{s}");
        assert!(s.contains("fleet score_v3 (52 agents)"), "recommendation missing:\n{s}");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ccwatch-tui ui::tests::renders_cruise_advisory_when_plan_proposes_pauses`
Expected: FAIL — the advisory text isn't rendered.

- [ ] **Step 3: Add `pacing_advisory` and wire it in**

In `tui/src/ui.rs`, add near `governor_estimated`:

```rust
/// A one-line Cruise Control recommendation when the pacing plan proposes pauses.
/// Advisory only — nothing is executed. `None` when there's no plan or nothing to do.
fn pacing_advisory(app: &App) -> Option<String> {
    let p = app.snapshot.pacing.as_ref()?;
    if p.actions.is_empty() {
        return None;
    }
    // Name the first recommended target; summarise the rest as "+N more".
    let first = p.actions.iter().find_map(|a| match a {
        ccwatch_core::model::PaceAction::Pause { reason, .. } => Some(reason.clone()),
        _ => None,
    })?;
    let more = p.actions.len().saturating_sub(1);
    let tail = if more > 0 { format!(" (+{more} more)") } else { String::new() };
    Some(format!("Cruise · to coast: {first}{tail}"))
}
```

In `draw` (the `alert_h` line), add the advisory to the row count so it has space:

```rust
    let extra = governor_estimated(app) as usize + pacing_advisory(app).is_some() as usize;
    let alert_h = ((app.snapshot.alerts.len() + extra) as u16).clamp(1, 6) + 2;
```

In `draw_alerts`, prepend the advisory line (after the `governor_estimated` tip block, before the alerts are extended). Find where the `governor_estimated` `items.push(...)` block ends and add right after it:

```rust
    if let Some(text) = pacing_advisory(app) {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("⏸ ", Style::default().fg(Color::Cyan)),
            Span::styled(text, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ])));
    }
```

(If `draw_alerts` currently early-returns "no leaks detected" only when `items.is_empty()`, this still works — the advisory makes `items` non-empty and it renders in the list.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ccwatch-tui ui::tests::`
Expected: the new test and all existing ui tests pass.

- [ ] **Step 5: Clippy + commit**

Run: `cargo clippy -p ccwatch-tui --all-targets -- -D warnings` (expect clean), then:

```bash
git add tui/src/ui.rs
git commit -m "feat(tui): Cruise Control advisory line (read-only)"
```

---

### Task 2: Swift model — decode the pacing plan

Give the app the Decodable types so `snap.pacing` deserializes.

**Files:**
- Modify: `app/Sources/Redline/Models.swift`

**Interfaces:**
- Produces: `struct PacingPlan: Decodable { targetRate, actualRate, price: Double; actions: [PaceAction]; reason: String }`, `struct PaceAction: Decodable { op: String; pid: Int?; reason: String? }`, and `Snapshot.pacing: PacingPlan?`.

- [ ] **Step 1: Add the types**

In `app/Sources/Redline/Models.swift`, add (match the existing `Decodable` + default-value style used by `Tank`/`GovernorStatus`; the Rust side is snake_case, so the app's `JSONDecoder` — check `Store.swift` — uses `.convertFromSnakeCase`; if it does NOT, add explicit `CodingKeys`):

```swift
struct PaceAction: Decodable {
    var op: String = ""        // "pause" | "resume"
    var pid: Int?
    var reason: String?
}

struct PacingPlan: Decodable {
    var targetRate: Double = 0
    var actualRate: Double = 0
    var price: Double = 0
    var actions: [PaceAction] = []
    var reason: String = ""
}
```

And add to `struct Snapshot`:

```swift
    var pacing: PacingPlan?
```

- [ ] **Step 2: Verify snake_case decoding**

Run: `grep -n "convertFromSnakeCase\|keyDecodingStrategy\|JSONDecoder" app/Sources/Redline/Store.swift`
- If `.convertFromSnakeCase` is set, the field names above (`targetRate`, `actualRate`) decode from `target_rate`/`actual_rate` automatically — done.
- If NOT set, add explicit `CodingKeys` to `PacingPlan` mapping `targetRate = "target_rate"`, `actualRate = "actual_rate"`, and to `PaceAction` (its keys are already lowercase single words, no mapping needed). Mirror however `Tank` (which has `budgetSource`/`windowStart`) already handles this — copy that exact mechanism.

- [ ] **Step 3: Build**

Run: `swift build --package-path app`
Expected: `Build complete!`

- [ ] **Step 4: Commit**

```bash
git add app/Sources/Redline/Models.swift
git commit -m "feat(app): decode the pacing plan from the snapshot"
```

---

### Task 3: Swift app — Cruise advisory in the Governor header

Show the recommendation in the menu-bar popover, under the existing pace line.

**Files:**
- Modify: `app/Sources/Redline/Menu.swift`

**Interfaces:**
- Consumes: `snap.pacing: PacingPlan?` (Task 2).

- [ ] **Step 1: Add the advisory view**

In `app/Sources/Redline/Menu.swift`, inside the Governor `header(_ snap:)` builder, right after the existing `paceLine(g, snap.generatedAt)` call (still inside `if let g = snap.governor { ... }`), add:

```swift
                // Cruise Control advisory (read-only in this step): if the pacer
                // proposes pauses, show the first recommendation.
                if let plan = snap.pacing, !plan.actions.isEmpty,
                   let first = plan.actions.first(where: { $0.op == "pause" })?.reason {
                    Text("⏸ Cruise · \(first)")
                        .font(.caption2)
                        .foregroundStyle(Palette.teal)
                        .lineLimit(2)
                }
```

- [ ] **Step 2: Build**

Run: `swift build --package-path app`
Expected: `Build complete!`

- [ ] **Step 3: Manual verification (no unit test exists for SwiftUI)**

Because there's no Swift test target, verify by building the release app and confirming the line appears when a plan has actions. This requires a live over-target state, which may not exist on demand — so the acceptance check is: (a) the build succeeds; (b) code review confirms the line reads `snap.pacing.actions` and calls NO actuator (`store.killSession`/`pauseSession`/`resumeSession` must NOT appear in this block). Note this in the report.

- [ ] **Step 4: Commit**

```bash
git add app/Sources/Redline/Menu.swift
git commit -m "feat(app): Cruise Control advisory in the Governor header (read-only)"
```

---

## Self-Review

**Spec coverage (Advisory step):**
- TUI shows the pacing recommendation → Task 1 (TDD). ✓
- App decodes + shows the recommendation → Tasks 2–3 (build-verified). ✓
- Read-only (no enforcement) → enforced in all three tasks; called out in Task 3's review check. ✓

**Deferred (later steps / by design):** the burn-down trajectory chart (spec's richer Advisory visual — text advisory ships first), one-click buttons and the reservation picker (Step 3), and the autonomous loop (Step 4). The TUI test is the automated guard; SwiftUI is build- + review-verified since the repo has no SwiftUI test harness.

**Placeholder scan:** none — every step has concrete code or an exact command. The two "check how X already does it" notes (Task 2 snake_case, Task 3 manual check) are verification steps with named fallbacks.

**Type consistency:** `PacingPlan`/`PaceAction` fields match Step 1's Rust wire types (`target_rate`/`actual_rate`/`price`/`actions`/`reason`; `op`/`pid`/`reason`); `pacing_advisory(app) -> Option<String>` used consistently.
