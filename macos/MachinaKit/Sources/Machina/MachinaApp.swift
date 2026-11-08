import SwiftUI
import AppKit
import MachinaKit

@MainActor final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) { QuickAccess.shared.reconfigure() }

    func applicationDockMenu(_ sender: NSApplication) -> NSMenu? {
        do {
            let menu = NSMenu()
            let model = AppModel.shared
            guard model.phase == .ready else { menu.addItem(withTitle: "Not connected", action: nil, keyEquivalent: ""); return menu }
            menu.addItem(withTitle: "\(model.runningCount) of \(model.vms.count) VMs running", action: nil, keyEquivalent: "")
            for (title, link) in [("Fleet", DeepLink.fleet), ("Tasks", .tasks), ("Alerts", .alerts)] {
                let item = NSMenuItem(title: title, action: #selector(openLink(_:)), keyEquivalent: ""); item.target = self; item.representedObject = link.url
                menu.addItem(item)
            }
            menu.addItem(.separator())
            for vm in model.vms.prefix(12) {
                let item = NSMenuItem(title: "\(vm.isRunning ? "●" : "○") \(vm.name)", action: #selector(openLink(_:)), keyEquivalent: "")
                item.target = self; item.representedObject = DeepLink.vm(vm.name).url
                menu.addItem(item)
            }
            return menu
        }
    }

    @objc private func openLink(_ item: NSMenuItem) {
        guard let url = item.representedObject as? URL, let link = DeepLink(url: url) else { return }
        AppModel.shared.open(link)
    }
}

@main
struct MachinaApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var model = AppModel.shared

    var body: some Scene {
        Window("Machina", id: "main") {
            Group {
                if model.phase == .ready { MainWindow() } else { LoginView() }
            }
            .environment(model)
            .frame(minWidth: 760, minHeight: 480)
            .onOpenURL { model.openShared(url: $0) }
            .onContinueUserActivity(ShareLink.activityType) { activity in
                if let r = ShareLink.link(fromActivity: activity.userInfo) { model.open(r.link) }
            }
            .onContinueUserActivity(SpotlightIndexer.activityType) { activity in
                if let name = SpotlightIndexer.vmName(from: activity) { model.open(.vm(name)) }
            }
        }
        .commands { MachinaCommands(model: model) }
        .commands { UpdateCommands() }
        .commands { QuickAccessCommands(model: model) }
        .commands { AIOperatorCommands(model: model) }
        // One VM per window: opened from the sidebar's context menu.
        WindowGroup("VM", id: "vm-window", for: String.self) { $name in
            VMWindow(name: name ?? "").environment(model)
        }
        MenuBarExtra {
            MenuBarContent().environment(model)
        } label: {
            // Glyph follows fleet health: rack when clear, circle/triangle with open alerts, offline hosts or approvals.
            Image(systemName: model.phase == .ready ? model.health.symbol : "server.rack")
        }
        Settings { SettingsView().environment(model) }
    }
}

struct VMWindow: View {
    @Environment(AppModel.self) private var model
    let name: String
    @State private var confirmStop: VmInfo?
    var body: some View {
        Group {
            if let vm = model.vms.first(where: { $0.name == name }) { VMDetail(vm: vm, confirmStop: $confirmStop) }
            else if model.phase != .ready { ContentUnavailableView("Not connected", systemImage: "network.slash", description: Text("Sign in from the main window."))}
            else { ContentUnavailableView("VM not found", systemImage: "server.rack", description: Text(name)) }
        }
        .frame(minWidth: 640, minHeight: 440)
        .confirm(Bindable(model).pendingAction).prompt(Bindable(model).prompt)
        .navigationTitle(name)
    }
}
