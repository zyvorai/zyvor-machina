import SwiftUI
import AppKit
import MachinaKit

/// Manual update check against GitHub releases. Installing is opt-in and verified (`SelfUpdater`).
@MainActor enum Updates {
    static var currentVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0.0.0"
    }

    static func checkNow() async {
        let status = await UpdateChecker.check(current: currentVersion)
        let alert = NSAlert()
        switch status {
        case .upToDate(let v):
            alert.messageText = String(localized: "You're up to date")
            alert.informativeText = String(localized: "Machina \(v) is the latest version.")
            alert.addButton(withTitle: String(localized: "OK"))
            alert.runModal()
        case .available(let rel):
            alert.messageText = String(localized: "Machina \(rel.version) is available")
            alert.informativeText = String(localized: "You have \(currentVersion). Install now to download it, verify its checksum, signature and Gatekeeper status, and replace this app.")
            alert.addButton(withTitle: rel.dmgURL == nil ? String(localized: "Open Release Page") : String(localized: "Download and Install"))
            if rel.dmgURL != nil { alert.addButton(withTitle: String(localized: "Release Page")) }
            alert.addButton(withTitle: String(localized: "Later"))
            switch alert.runModal() {
            case .alertFirstButtonReturn:
                if rel.dmgURL != nil { await SelfUpdater.start(rel) } else { NSWorkspace.shared.open(rel.pageURL) }
            case .alertSecondButtonReturn where rel.dmgURL != nil: NSWorkspace.shared.open(rel.pageURL)
            default: break
            }
        case .failed(let why):
            alert.alertStyle = .warning
            alert.messageText = String(localized: "Could not check for updates")
            alert.informativeText = why
            alert.addButton(withTitle: String(localized: "OK"))
            alert.runModal()
        }
    }
}

struct UpdateCommands: Commands {
    var body: some Commands {
        CommandGroup(after: .appInfo) {
            Button("Check for Updates…") { Task { await Updates.checkNow() } }
        }
    }
}
