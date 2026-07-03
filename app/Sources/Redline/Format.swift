import SwiftUI

// The Redline visual language — same palette as the icon and the TUI.
enum Palette {
    static let teal   = Color(rgb: 48, 176, 199)
    static let orange = Color(rgb: 255, 159, 10)
    static let red    = Color(rgb: 255, 69, 58)
    static let green  = Color(rgb: 120, 205, 130)
    static let purple = Color(rgb: 181, 140, 255)
    static let cream  = Color(rgb: 246, 240, 225)
    static let ink    = Color(rgb: 20, 20, 20)
    static let dim    = Color.secondary
}

extension Color {
    init(rgb r: Double, _ g: Double, _ b: Double) {
        self.init(.sRGB, red: r / 255, green: g / 255, blue: b / 255, opacity: 1)
    }
}

/// The daemon marks an "empty tank / hard wall" with this sentinel delta.
let DELTA_EMPTY = 99.0

/// Which limit the Governor's headline number reflects. This changes the
/// *number*, not how it's displayed. "mix" = the binding limit (whichever
/// of the 5h window / weekly cap you'll hit first).
enum LimitMode: String, CaseIterable, Identifiable {
    case window, week, mix
    var id: String { rawValue }
    var short: String {
        switch self {
        case .window: return "5h window"
        case .week: return "Weekly"
        case .mix: return "Mix"
        }
    }
    /// Compact label for segmented controls.
    var seg: String {
        switch self {
        case .window: return "5h"
        case .week: return "Week"
        case .mix: return "Mix"
        }
    }
}

/// What the menu-bar text shows for the selected limit — orthogonal to which
/// limit is selected.
enum MenuShow: String, CaseIterable, Identifiable {
    case throttle, percent, rate, nothing
    var id: String { rawValue }
    var title: String {
        switch self {
        case .throttle: return "Throttle (▲/▼×)"
        case .percent: return "Percent used"
        case .rate: return "Burn rate"
        case .nothing: return "Nothing"
        }
    }
    /// Compact label for segmented controls.
    var seg: String {
        switch self {
        case .throttle: return "▲▼×"
        case .percent: return "%"
        case .rate: return "Rate"
        case .nothing: return "Off"
        }
    }
}

enum Gov {
    /// The binding tank — whichever wall you'll hit first (larger delta).
    static func binding(_ g: GovernorStatus) -> (tank: Tank, isWeek: Bool) {
        let w = g.window
        guard let wk = g.week else { return (w, false) }
        let wd = w.delta ?? 0, kd = wk.delta ?? 0
        return kd > wd ? (wk, true) : (w, false)
    }

    /// The tank the user picked (5h / weekly / binding-mix) plus a label.
    static func selected(_ g: GovernorStatus, _ mode: LimitMode) -> (tank: Tank, label: String) {
        switch mode {
        case .window: return (g.window, "5H WINDOW")
        case .week: return (g.week ?? g.window, "WEEKLY")
        case .mix:
            let b = binding(g)
            return (b.tank, "MIX · \(b.isWeek ? "WEEKLY" : "5H") BINDING")
        }
    }

    /// The menu-bar string for a tank under a display mode (nil = show nothing).
    static func trayText(_ t: Tank, _ show: MenuShow) -> String? {
        switch show {
        case .throttle: return throttleLabel(t.delta)
        case .percent: return t.usedFrac.map { String(format: "%.0f%%", $0 * 100) } ?? "—"
        case .rate: return Fmt.rate(t.ratePerMin)
        case .nothing: return nil
        }
    }

    /// Throttle heat 0…1 → color ramp teal → orange → red.
    static func heat(_ delta: Double?) -> Double {
        guard let d = delta else { return 0 }
        if d >= DELTA_EMPTY { return 1 }
        return min(1, max(0, (d - 0.6) / 0.4))
    }

    static func heatColor(_ h: Double) -> Color {
        if h <= 0.5 {
            return blend(Palette.teal, Palette.orange, h / 0.5)
        }
        return blend(Palette.orange, Palette.red, (h - 0.5) / 0.5)
    }

    /// "▲2.1×" over budget, "▼0.6×" coasting, "▼0.05×" barely burning, "⛔" at
    /// the wall. Small deltas use two decimals so they don't collapse to "0.0×".
    static func throttleLabel(_ delta: Double?) -> String {
        guard let d = delta else { return "—" }
        if d >= DELTA_EMPTY { return "⛔" }
        let arrow = d >= 1 ? "▲" : "▼"
        let num = (d > 0 && d < 0.095)
            ? String(format: "%.2f", d)
            : String(format: "%.1f", d)
        return "\(arrow)\(num)×"
    }

    private static func blend(_ a: Color, _ b: Color, _ t: Double) -> Color {
        let t = min(1, max(0, t))
        let ca = a.rgba, cb = b.rgba
        return Color(.sRGB,
                     red: ca.0 + (cb.0 - ca.0) * t,
                     green: ca.1 + (cb.1 - ca.1) * t,
                     blue: ca.2 + (cb.2 - ca.2) * t,
                     opacity: 1)
    }
}

extension Color {
    var rgba: (Double, Double, Double, Double) {
        let n = NSColor(self).usingColorSpace(.sRGB) ?? .white
        return (Double(n.redComponent), Double(n.greenComponent),
                Double(n.blueComponent), Double(n.alphaComponent))
    }
}

enum Fmt {
    static func tokens(_ n: UInt64) -> String {
        let d = Double(n)
        if d >= 1_000_000 { return String(format: "%.1fM", d / 1_000_000) }
        if d >= 1_000 { return String(format: "%.0fk", d / 1_000) }
        return "\(n)"
    }

    static func rate(_ tpm: Double) -> String {
        if tpm <= 0 { return "—" }
        if tpm >= 1000 { return String(format: "%.1fk/min", tpm / 1000) }
        return String(format: "%.0f/min", tpm)
    }

    /// Epoch-ms → local "HH:mm".
    static func clock(_ epochMs: Int64?) -> String {
        guard let ms = epochMs, ms > 0 else { return "—" }
        let df = DateFormatter()
        df.dateFormat = "HH:mm"
        return df.string(from: Date(timeIntervalSince1970: Double(ms) / 1000))
    }

    /// Epoch-ms → "HH:mm" if within ~18h, else "Wed Jul 9, 20:00" — so a
    /// near-term time is compact but a days-away one carries its date.
    static func smartTime(_ epochMs: Int64, now nowMs: Int64) -> String {
        (epochMs - nowMs) < 18 * 3_600_000 ? clock(epochMs) : resetLabel(epochMs)
    }

    /// Epoch-ms → local "Wed Jul 9, 20:00" (weekday + date + time) for the
    /// weekly reset, which can be days away.
    static func resetLabel(_ epochMs: Int64?) -> String {
        guard let ms = epochMs, ms > 0 else { return "—" }
        let df = DateFormatter()
        df.dateFormat = "EEE MMM d, HH:mm"
        return df.string(from: Date(timeIntervalSince1970: Double(ms) / 1000))
    }

    /// A duration-in-ms → "3m", "2h", "12s".
    static func ago(_ ms: Int64) -> String {
        let s = ms / 1000
        if s < 60 { return "\(s)s" }
        if s < 3600 { return "\(s / 60)m" }
        if s < 86400 { return "\(s / 3600)h" }
        return "\(s / 86400)d"
    }

    static func tierColor(_ tier: String) -> Color {
        switch tier.lowercased() {
        case "opus": return Palette.teal
        case "fable": return Palette.purple
        case "sonnet": return Palette.orange
        case "haiku": return Palette.green
        default: return Palette.dim
        }
    }

    static func stateColor(_ state: String) -> Color {
        switch state.lowercased() {
        case "running": return Palette.green
        case "waiting": return Palette.orange
        case "idle": return Palette.dim
        case "ended", "finished": return Palette.dim
        default: return Palette.dim
        }
    }
}
