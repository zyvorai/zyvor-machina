import SwiftUI
import AppKit
import Carbon.HIToolbox
import UserNotifications
import MachinaKit

/// Opt-in "reach Machina from anywhere" features. Nothing here runs unless the user turned it on in Settings
/// (global hotkey and digest default to off) and nothing leaves the Mac: the digest is built from data the app
/// already fetched from the saved server and shown as a local notification. No feature here changes a server.

@MainActor @Observable
final class QuickAccessSettings {
    static let shared = QuickAccessSettings()
    private let d = UserDefaults.standard

    var hotKeyEnabled: Bool { didSet { d.set(hotKeyEnabled, forKey: "qa.hotKeyEnabled"); QuickAccess.shared.reconfigure() } }
    var hotKey: HotKeySpec { didSet { d.set(try? JSONEncoder().encode(hotKey), forKey: "qa.hotKey"); QuickAccess.shared.reconfigure() } }
    var digest: DigestSettings { didSet { d.set(try? JSONEncoder().encode(digest), forKey: "qa.digest"); QuickAccess.shared.reconfigure() } }
    var lastDigest: Date? { didSet { d.set(lastDigest, forKey: "qa.lastDigest") } }

    private init() {
        hotKeyEnabled = UserDefaults.standard.bool(forKey: "qa.hotKeyEnabled")
        hotKey = UserDefaults.standard.data(forKey: "qa.hotKey").flatMap { try? JSONDecoder().decode(HotKeySpec.self, from: $0) }.flatMap { $0.isValid ? $0 : nil } ?? .default
        digest = UserDefaults.standard.data(forKey: "qa.digest").flatMap { try? JSONDecoder().decode(DigestSettings.self, from: $0) } ?? DigestSettings()
        lastDigest = UserDefaults.standard.object(forKey: "qa.lastDigest") as? Date
    }
}

@MainActor
final class QuickAccess {
    static let shared = QuickAccess()
    private var hotKeyRef: EventHotKeyRef?
    private var handlerRef: EventHandlerRef?
    private var digestTask: Task<Void, Never>?
    private(set) var hotKeyError: String?

    /// Applies the current settings: (un)registers the hotkey and (re)starts the digest loop. Safe to call often.
    func reconfigure() {
        let s = QuickAccessSettings.shared
        unregisterHotKey()
        if s.hotKeyEnabled { registerHotKey(s.hotKey) }
        digestTask?.cancel(); digestTask = nil
        if s.digest.enabled { digestTask = Task { await digestLoop() } }
    }

    // MARK: Hotkey (Carbon RegisterEventHotKey: needs no Accessibility or Input Monitoring permission)

    private func registerHotKey(_ spec: HotKeySpec) {
        hotKeyError = nil
        guard spec.isValid else { hotKeyError = "Not a valid shortcut"; return }
        if handlerRef == nil {
            var type = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
            let status = InstallEventHandler(GetApplicationEventTarget(), { _, _, _ in
                DispatchQueue.main.async { MainActor.assumeIsolated { QuickAccess.shared.hotKeyPressed() } }
                return noErr
            }, 1, &type, nil, &handlerRef)
            if status != noErr { hotKeyError = "Could not install the shortcut handler (\(status))"; return }
        }
        let id = EventHotKeyID(signature: OSType(0x4D434E41) /* MCNA */, id: 1)
        let status = RegisterEventHotKey(spec.keyCode, spec.modifiers, id, GetApplicationEventTarget(), 0, &hotKeyRef)
        if status != noErr { hotKeyRef = nil; hotKeyError = "Another app already uses \(spec.label)" }
    }

    private func unregisterHotKey() {
        if let hotKeyRef { UnregisterEventHotKey(hotKeyRef) }
        hotKeyRef = nil
    }

    /// Brings Machina forward and opens the Cmd-K palette. Only when signed in: the palette needs the VM list.
    func hotKeyPressed() {
        let model = AppModel.shared
        NSApp.activate(ignoringOtherApps: true)
        if let w = NSApp.windows.first(where: { $0.identifier?.rawValue.contains("main") == true || $0.title == "Machina" }) { w.makeKeyAndOrderFront(nil) }
        if model.phase == .ready { model.showPalette = true }
    }

    // MARK: Digest

    /// Wakes every 30 s (so sleep, clock changes and relaunches are all handled) and posts once per day after the chosen time.
    private func digestLoop() async {
        while !Task.isCancelled {
            await digestTick(now: Date())
            try? await Task.sleep(nanoseconds: 30_000_000_000)
        }
    }

    private func digestTick(now: Date) async {
        let s = QuickAccessSettings.shared
        let model = AppModel.shared
        guard s.digest.enabled, model.phase == .ready else { return }
        // The most recent occurrence of the chosen time, at or before now.
        let next = DigestBuilder.nextFire(after: now, minuteOfDay: s.digest.minuteOfDay)
        let due = Calendar.current.date(byAdding: .day, value: -1, to: next) ?? next
        guard due <= now, !DigestBuilder.alreadyPosted(lastPosted: s.lastDigest, due: due), (s.lastDigest ?? .distantPast) < due else { return }
        await model.refreshPlatform()
        let digest = DigestBuilder.build(alerts: model.alerts, tasks: model.tasks, since: due.addingTimeInterval(-Double(s.digest.lookbackHours) * 3600))
        switch DigestBuilder.delivery(for: digest, quiet: model.settings.quietHours, at: now) {
        case .deferUntil: return   // still inside quiet hours; the next tick tries again
        case .now:
            let content = UNMutableNotificationContent()
            content.title = "Machina overnight: " + digest.title
            content.body = digest.body
            content.userInfo = ["digestLink": digest.link.url.absoluteString]
            _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound])
            try? await UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: "digest-\(Int(due.timeIntervalSince1970))", content: content, trigger: nil))
            s.lastDigest = now
        }
    }
}

// MARK: Handoff and link sharing

extension AppModel {
    /// `machina://vm/<name>?server=<host>` for the selected VM. Carries no credentials.
    func shareURL(_ link: DeepLink) -> URL { ShareLink.url(for: link, server: URL(string: serverURL)) }

    func copyShareLink() {
        guard case .vm(let n)? = selection else { notice = "Select a VM first"; return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(shareURL(.vm(n)).absoluteString, forType: .string)
        notice = "Copied link to \(n)"
    }

    /// Opens a VM from a Handoff activity or pasted link; warns when the link was made on a different server.
    func openShared(url: URL) {
        guard let link = DeepLink(url: url) else { return }
        if case .different(let other) = ShareLink.match(link: url, current: URL(string: serverURL)) {
            notice = "This link is for \(other); you are signed in to a different server"
        }
        open(link)
    }
}

struct HandoffAdvertiser: ViewModifier {
    @Environment(AppModel.self) private var model
    let vm: String
    func body(content: Content) -> some View {
        content.userActivity(ShareLink.activityType) { a in
            a.title = vm
            a.isEligibleForHandoff = true
            a.userInfo = ShareLink.activityInfo(vm: vm, server: URL(string: model.serverURL))
            a.needsSave = true
        }
    }
}
extension View { func advertisesHandoff(vm: String) -> some View { modifier(HandoffAdvertiser(vm: vm)) } }

// MARK: Floating glance (always-on-top, read-only)

struct GlanceView: View {
    @Environment(AppModel.self) private var model
    let name: String
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if let vm = model.vms.first(where: { $0.name == name }) {
                let g = VmGlance.make(vm: vm, alerts: model.alerts, tasks: model.tasks)
                HStack {
                    Image(systemName: "circle.fill").font(.caption2).foregroundStyle(g.level == .ok ? .green : g.level == .warning ? .orange : .red)
                    Text(g.name).font(.headline)
                }
                Text(g.stateLine).font(.caption).foregroundStyle(.secondary)
                ForEach(g.notes, id: \.self) { Text($0).font(.caption2).lineLimit(2) }
            } else {
                Text(name).font(.headline); Text("Not available").font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(10).frame(width: 240, alignment: .leading)
    }
}

@MainActor enum GlancePanel {
    private static var panels: [String: NSPanel] = [:]

    static func toggle(vm: String, model: AppModel) {
        if let p = panels[vm] { p.close(); panels[vm] = nil; return }
        let p = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 240, height: 90),
                        styleMask: [.titled, .closable, .utilityWindow, .nonactivatingPanel, .hudWindow], backing: .buffered, defer: false)
        p.title = vm; p.level = .floating; p.isFloatingPanel = true; p.hidesOnDeactivate = false
        p.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        p.isReleasedWhenClosed = false
        p.contentView = NSHostingView(rootView: GlanceView(name: vm).environment(model))
        p.setContentSize(p.contentView?.fittingSize ?? NSSize(width: 240, height: 90))
        p.center(); p.orderFrontRegardless()
        NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: p, queue: .main) { _ in
            MainActor.assumeIsolated { panels[vm] = nil }
        }
        panels[vm] = p
    }
}

struct QuickAccessCommands: Commands {
    var model: AppModel
    var body: some Commands {
        CommandMenu("Share") {
            Button("Copy Link to Selected VM") { model.copyShareLink() }.keyboardShortcut("l", modifiers: [.command, .shift])
            Button("Float Selected VM on Top") {
                if case .vm(let n)? = model.selection { GlancePanel.toggle(vm: n, model: model) } else { model.notice = "Select a VM first" }
            }.keyboardShortcut("f", modifiers: [.command, .option])
        }
    }
}

// MARK: Settings tab

struct QuickAccessSettingsForm: View {
    @State private var s = QuickAccessSettings.shared
    var body: some View {
        Form {
            Section("Global shortcut") {
                Toggle("Open the command palette from any app", isOn: $s.hotKeyEnabled)
                Picker("Shortcut", selection: $s.hotKey) {
                    ForEach(HotKeySpec.presets, id: \.self) { Text($0.label).tag($0) }
                }.disabled(!s.hotKeyEnabled)
                if s.hotKeyEnabled, let e = QuickAccess.shared.hotKeyError { Text(e).font(.caption).foregroundStyle(.red) }
                Text("Off by default. Needs no extra macOS permission. Opens the palette only while you are signed in.").font(.caption).foregroundStyle(.secondary)
            }
            Section("Morning digest") {
                Toggle("Summarise overnight alerts and failed tasks", isOn: $s.digest.enabled)
                Picker("Post at", selection: $s.digest.minuteOfDay) {
                    ForEach(Array(stride(from: 0, to: 24 * 60, by: 30)), id: \.self) { Text(QuietHours.label($0)).tag($0) }
                }.disabled(!s.digest.enabled)
                Picker("Look back", selection: $s.digest.lookbackHours) {
                    ForEach([8, 12, 14, 24, 48], id: \.self) { Text("\($0) hours").tag($0) }
                }.disabled(!s.digest.enabled)
                Text("Off by default. Machina must be running and signed in at that time. Quiet hours apply: a digest with nothing critical waits until they end. It is a local notification built from data the app already has.").font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}
