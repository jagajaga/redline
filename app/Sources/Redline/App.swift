import SwiftUI
import AppKit

@main
struct RedlineApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    var body: some Scene {
        MenuBarExtra {
            MenuContent(store: appDelegate.store, delegate: appDelegate)
        } label: {
            TrayLabel(store: appDelegate.store)
        }
        .menuBarExtraStyle(.window)
    }
}

/// Owns the shared store and the dashboard window, and manages the
/// dock/tray activation policy (regular when the window is up, accessory
/// when it's been closed to the menu bar).
// Redline is a menu-bar app (LSUIElement): always accessory, never a dock icon.
// We never toggle activation policy — doing so tears down the MenuBarExtra —
// so the status item is always present. The dashboard is a normal window we
// show/hide on demand.
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    let store = Store()
    private var window: NSWindow?

    func applicationDidFinishLaunching(_ notification: Notification) {
        store.start()
        if UserDefaults.standard.bool(forKey: "start_menu_bar_only") {
            // Menu-bar only: no Dock icon.
            NSApp.setActivationPolicy(.accessory)
        } else {
            showDashboard()
        }
    }

    func showDashboard() {
        if window == nil { makeWindow() }
        // Dock icon appears while the window is open.
        NSApp.setActivationPolicy(.regular)
        window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    private func makeWindow() {
        let host = NSHostingController(rootView: DashboardView(store: store))
        let w = NSWindow(contentViewController: host)
        w.setContentSize(NSSize(width: 940, height: 760))
        w.title = "Redline"
        w.titlebarAppearsTransparent = true
        w.titleVisibility = .hidden
        w.styleMask.insert(.fullSizeContentView)
        w.isMovableByWindowBackground = true
        w.isReleasedWhenClosed = false
        w.center()
        w.delegate = self
        window = w
    }

    // Closing the window drops the Dock icon (menu-bar only).
    func windowWillClose(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
    }

    // Relaunching the app (Finder/open) brings the window back.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        showDashboard()
        return true
    }

    // Launch the bundled ccwatch TUI in Terminal.
    func openTUI() {
        guard let exe = Bundle.main.executablePath else { return }
        let tui = (exe as NSString).deletingLastPathComponent + "/ccwatch"
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/open")
        p.arguments = ["-a", "Terminal", tui]
        try? p.run()
    }

    func quit() { NSApp.terminate(nil) }
}

/// True desktop vibrancy behind the SwiftUI content.
struct VisualEffectView: NSViewRepresentable {
    var material: NSVisualEffectView.Material = .underWindowBackground

    func makeNSView(context: Context) -> NSVisualEffectView {
        let v = NSVisualEffectView()
        v.material = material
        v.blendingMode = .behindWindow
        v.state = .active
        return v
    }

    func updateNSView(_ v: NSVisualEffectView, context: Context) {
        v.material = material
    }
}
