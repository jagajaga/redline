import Foundation
import ServiceManagement

/// Registers/unregisters Redline as a macOS login item via SMAppService
/// (the modern, sandbox-friendly API; macOS 13+).
enum LoginItem {
    static var enabled: Bool {
        SMAppService.mainApp.status == .enabled
    }

    static func set(_ on: Bool) {
        do {
            if on {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
        } catch {
            NSLog("Redline: login item toggle failed: \(error.localizedDescription)")
        }
    }
}
