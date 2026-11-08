import SwiftUI
import AppKit
import MachinaKit

/// Manual update check against GitHub releases. Never downloads or installs anything: it shows the
/// result and offers to open the release's DMG (or the release page) in the browser.
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
            alert.informativeText = String(localized: "You have \(currentVersion). Updates are not installed automatically: download the new version and replace the app.")
            alert.addButton(withTitle: rel.dmgURL == nil ? String(localized: "Open Release Page") : String(localized: "Download DMG"))
            if rel.dmgURL != nil { alert.addButton(withTitle: String(localized: "Release Page")) }
            alert.addButton(withTitle: String(localized: "Later"))
            switch alert.runModal() {
            case .alertFirstButtonReturn: NSWorkspace.shared.open(rel.dmgURL ?? rel.pageURL)
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
