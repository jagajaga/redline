import SwiftUI

/// The window's session list: click-anywhere-to-expand cards, each revealing
/// everything the session is doing — actions, agents (nested), tasks,
/// processes, and watchers.
struct SessionTree: View {
    let snap: Snapshot
    let store: Store
    var hideDone = false
    var hideInactive = false

    var body: some View {
        let sessions = snap.sessions
            .filter { keep($0) }
            .sorted { $0.tokensPerMin > $1.tokensPerMin }
        if sessions.isEmpty {
            let hiding = (hideInactive || hideDone) && !snap.sessions.isEmpty
            VStack(spacing: 4) {
                Text(hiding ? "Nothing active right now" : "No sessions")
                    .foregroundStyle(.secondary)
                if hiding {
                    Text("Uncheck the filters below to show all \(snap.sessions.count)")
                        .font(.caption).foregroundStyle(.tertiary)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(sessions) { s in
                        SessionDetail(session: s, store: store, hideDone: hideDone)
                        Divider().opacity(0.5)
                    }
                }
            }
        }
    }

    private func isActive(_ s: Session) -> Bool {
        let st = s.state.lowercased()
        return st == "running" || st == "waiting" || s.tokensPerMin > 0
    }
    private func keep(_ s: Session) -> Bool {
        let st = s.state.lowercased()
        if hideDone && st == "ended" { return false }
        if hideInactive && !isActive(s) { return false }
        return true
    }
}

/// One session, expandable to a full breakdown of its work.
struct SessionDetail: View {
    let session: Session
    let store: Store
    var hideDone = false
    @State private var expanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button { withAnimation(.easeInOut(duration: 0.12)) { expanded.toggle() } } label: {
                headerRow
            }
            .buttonStyle(.plain)

            if expanded { detail }
        }
        .padding(.horizontal, 20).padding(.vertical, 9)
        .contentShape(Rectangle())
    }

    private var headerRow: some View {
        let s = session
        return HStack(spacing: 8) {
            Image(systemName: "chevron.right").font(.system(size: 10)).foregroundStyle(.secondary)
                .rotationEffect(.degrees(expanded ? 90 : 0))
            Circle().fill(Fmt.stateColor(s.state)).frame(width: 8, height: 8)
            Text(s.title ?? s.name).fontWeight(.medium).lineLimit(1)
            if let m = s.model {
                Text(tier(m)).font(.caption).foregroundStyle(Fmt.tierColor(tier(m)))
            }
            Text(hostLabel(s)).font(.caption).foregroundStyle(.secondary)
            Spacer()
            Text(s.tokensPerMin > 0 ? Fmt.rate(s.tokensPerMin) : s.state.lowercased())
                .font(.caption.monospacedDigit()).foregroundStyle(.secondary)
            Text(Fmt.tokens(s.tokens.total)).font(.caption.monospacedDigit()).frame(width: 60, alignment: .trailing)
        }
        .contentShape(Rectangle())
    }

    // MARK: - Everything the session is doing

    @ViewBuilder
    private var detail: some View {
        let s = session
        VStack(alignment: .leading, spacing: 8) {
            // Vitals
            HStack(spacing: 10) {
                if s.tokensPerMin > 0 {
                    Text(Fmt.rate(s.tokensPerMin)).foregroundStyle(Palette.teal)
                }
                Text("in \(Fmt.tokens(s.tokens.input)) · out \(Fmt.tokens(s.tokens.output)) · cache \(Fmt.tokens(s.tokens.cacheRead)) · \(s.tokens.messages) msgs")
                Text(String(format: "%.0f%% cpu · %llu MB", s.cpuPct, s.rssMb))
                if let st = s.startedAt { Text("started \(Fmt.clock(st))") }
                if let la = s.lastActivity { Text("last \(Fmt.clock(la))") }
            }.font(.caption2).foregroundStyle(.secondary)

            // Show only ACTIVE (running) subagents — the work happening now.
            let activeAgents = flatten(s.agents).filter { $0.2.state.lowercased() == "running" }
            let tasks = s.tasks.filter { !(hideDone && $0.status.lowercased() == "completed") }

            section("ACTIONS", s.activity.isEmpty) {
                ForEach(Array(s.activity.enumerated()), id: \.offset) { _, a in
                    row("wrench.and.screwdriver", Palette.teal) {
                        Text(a.tool).fontWeight(.medium)
                        Text(a.detail).foregroundStyle(.secondary).lineLimit(1)
                        Spacer(); Text(Fmt.ago(a.sinceMs)).foregroundStyle(.tertiary)
                    }
                }
            }
            section("ACTIVE AGENTS", activeAgents.isEmpty) {
                ForEach(activeAgents, id: \.0) { key, depth, a in
                    row("bolt.fill", Palette.green, indent: depth) {
                        Text(a.subagentType.isEmpty ? "agent" : a.subagentType).fontWeight(.medium)
                        if let m = a.model { Text(tier(m)).foregroundStyle(Fmt.tierColor(tier(m))) }
                        if let act = a.activity.first { Text("· \(act.tool) \(act.detail)").foregroundStyle(.secondary).lineLimit(1) }
                        Spacer()
                        if a.tokensPerMin > 0 { Text(Fmt.rate(a.tokensPerMin)).foregroundStyle(.secondary) }
                        Text(Fmt.tokens(a.tokens.total)).foregroundStyle(.tertiary)
                    }.id(key)
                }
            }
            section("TASKS", tasks.isEmpty) {
                ForEach(Array(tasks.enumerated()), id: \.offset) { _, t in
                    row(taskIcon(t), taskColor(t)) {
                        Text(t.activeForm ?? t.subject).lineLimit(1)
                        if t.blocked { Text("blocked").foregroundStyle(Palette.red) }
                        Spacer()
                    }
                }
            }
            section("PROCESSES", s.processes.isEmpty) {
                ForEach(s.processes) { p in
                    row("gearshape", .secondary) {
                        Text(p.name).fontWeight(.medium)
                        Text(p.cmd).foregroundStyle(.secondary).lineLimit(1)
                        Spacer()
                        Text(String(format: "%.0f%% · %llus", p.cpuPct, p.runSecs)).foregroundStyle(.tertiary)
                    }
                }
            }
            section("WATCHERS", s.watchers.isEmpty) {
                ForEach(Array(s.watchers.enumerated()), id: \.offset) { _, w in
                    row("eye", Palette.purple) {
                        Text(w.name).fontWeight(.medium)
                        Text(w.detail).foregroundStyle(.secondary).lineLimit(1)
                        Spacer()
                        if w.firedCount > 0 { Text("×\(w.firedCount)").foregroundStyle(.tertiary) }
                    }
                }
            }

            if s.activity.isEmpty && activeAgents.isEmpty && tasks.isEmpty && s.processes.isEmpty && s.watchers.isEmpty {
                let working = s.state.lowercased() == "running" || s.tokensPerMin > 0
                if working {
                    Text("Working at \(Fmt.rate(s.tokensPerMin)) — heavy subagent/tool work not yet broken out per-agent")
                        .font(.caption2).foregroundStyle(Palette.green)
                } else {
                    Text("Idle — nothing running right now")
                        .font(.caption2).foregroundStyle(.tertiary)
                }
            }

            if let pid = s.pid {
                HStack(spacing: 14) {
                    Button("Kill") { store.killSession(pid: pid) }
                    Button("Pause") { store.pauseSession(pid: pid) }
                    Button("Resume") { store.resumeSession(pid: pid) }
                    Spacer()
                    Text("pid \(pid)").foregroundStyle(.tertiary)
                }
                .buttonStyle(.borderless).font(.caption2).foregroundStyle(Palette.teal)
                .padding(.top, 2)
            }
        }
        .padding(.leading, 26).padding(.top, 2)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    // MARK: - Building blocks

    @ViewBuilder
    private func section<C: View>(_ title: String, _ isEmpty: Bool, @ViewBuilder _ content: () -> C) -> some View {
        if !isEmpty {
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.system(size: 9).weight(.bold)).foregroundStyle(.tertiary).tracking(0.5)
                content()
            }
        }
    }

    private func row<C: View>(_ icon: String, _ color: Color, indent: Int = 0, @ViewBuilder _ content: () -> C) -> some View {
        HStack(spacing: 5) {
            Image(systemName: icon).font(.system(size: 9)).foregroundStyle(color).frame(width: 12)
            content()
        }
        .font(.caption2)
        .padding(.leading, CGFloat(indent) * 12)
    }

    private func flatten(_ agents: [Agent], depth: Int = 0, prefix: String = "") -> [(String, Int, Agent)] {
        var out: [(String, Int, Agent)] = []
        for (i, a) in agents.enumerated() {
            let key = "\(prefix)\(i)-\(a.id)"
            out.append((key, depth, a))
            out.append(contentsOf: flatten(a.children, depth: depth + 1, prefix: key + "/"))
        }
        return out
    }
    private func tier(_ m: String) -> String {
        let l = m.lowercased()
        if l.contains("opus") { return "opus" }
        if l.contains("fable") { return "fable" }
        if l.contains("sonnet") { return "sonnet" }
        if l.contains("haiku") { return "haiku" }
        return "model"
    }
    private func hostLabel(_ s: Session) -> String {
        s.host.kind == "local" ? "local" : (s.host.name ?? s.remoteName ?? s.host.kind)
    }
    private func taskIcon(_ t: Task) -> String {
        switch t.status.lowercased() {
        case "completed": return "checkmark.circle.fill"
        case "in_progress": return "circle.lefthalf.filled"
        default: return "circle"
        }
    }
    private func taskColor(_ t: Task) -> Color {
        switch t.status.lowercased() {
        case "completed": return Palette.green
        case "in_progress": return Palette.teal
        default: return Palette.dim
        }
    }
}
