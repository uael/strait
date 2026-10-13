import ServiceManagement
import SwiftUI

@main
struct StraitApp: App {
    @StateObject private var engine = Engine()

    var body: some Scene {
        MenuBarExtra {
            MenuView()
                .environmentObject(engine)
        } label: {
            Image(systemName: menuBarSymbol)
        }
        .menuBarExtraStyle(.window)
    }

    private var menuBarSymbol: String {
        guard let status = engine.status else { return "rectangle.split.2x1" }
        if case .connected = status.link {
            return status.focus == .mac ? "rectangle.split.2x1" : "rectangle.righthalf.filled"
        }
        return "rectangle.split.2x1.slash"
    }
}

/// Opening at login, through the system's login items.
enum LoginItem {
    static var enabled: Bool { SMAppService.mainApp.status == .enabled }

    static func set(_ on: Bool) {
        do {
            if on { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() }
        } catch {
            NSLog("strait: login item: \(error)")
        }
    }
}
