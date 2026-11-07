import Foundation
import Observation
import MachinaKit
import UserNotifications
import AppKit

/// What the on-screen resource page exposes to menu commands.
struct PageContext {
    var spec: ResourceSpec
    var columns: [ResourceColumn]
    var rows: [ResourceRow]
}

enum SidebarItem: Hashable { case fleet, tasks, alerts, vm(String), page(String) }

@MainActor @Observable
final class AppModel {
    /// One model for the whole process so the Dock menu, App Intents and Spotlight handlers can reach it.
    static let shared = AppModel()

    enum Phase { case signedOut, connecting, ready }

    var phase: Phase = .signedOut
    var vms: [VmInfo] = [] { didSet { if oldValue != vms { SpotlightIndexer.shared.schedule(vms) } } }
    var error: String?
    var live = false
    var pendingFingerprint: String?
    var selection: SidebarItem?

    // Fleet / tasks / alerts (controller + fleet endpoints)
    var hosts: [PlatformHost] = []
    var tasks: [PlatformTask] = []
    var alerts: [FleetAlert] = []
    var unackedAlerts = 0
    var metrics: LocalMetrics?
    var history: [MetricsPoint] = []
    var platformError: String?
    var platformLoaded = false

    // Session role (admin / operator / readonly) from GET /auth/session; gates write controls.
    var role: String?
    var canWrite: Bool { role == "admin" || role == "operator" }
    var showPalette = false
    var showShortcuts = false
    var showBulk = false

    // Productivity state, kept in UserDefaults per Mac (names only; nothing secret).
    var pins: [String] = UserDefaults.standard.stringArray(forKey: "pinnedVMs") ?? [] { didSet { UserDefaults.standard.set(pins, forKey: "pinnedVMs") } }
    var recents: [String] = UserDefaults.standard.stringArray(forKey: "paletteRecents") ?? [] { didSet { UserDefaults.standard.set(recents, forKey: "paletteRecents") } }
    var savedFilters: [SavedFilter] = (UserDefaults.standard.data(forKey: "savedFilters")).flatMap { try? JSONDecoder().decode([SavedFilter].self, from: $0) } ?? [] {
        didSet { UserDefaults.standard.set(try? JSONEncoder().encode(savedFilters), forKey: "savedFilters") }
    }
    /// The sidebar's VM filter text (name words plus `is:running`, `vcpus>=4`, …).
    var vmQuery = ""
    /// The resource page on screen, so menu shortcuts can export it or copy its request as curl.
    var page: PageContext?

    func togglePin(_ name: String) { pins = Pins.toggling(name, in: pins) }
    func remember(_ id: String) { recents = Recents.pushing(id, onto: recents) }
    func saveCurrentFilter() {
        guard !VMFilter(vmQuery).isEmpty else { error = "Type a filter in the sidebar first (for example is:running vcpus>=4)."; return }
        ask("Save VM filter", message: vmQuery, submit: "Save", fields: [PromptField(id: "name", label: "Name")]) { [self] v in
            savedFilters = SavedFilter.saving(SavedFilter(name: v["name"] ?? "", query: vmQuery), into: savedFilters)
            notice = "Filter saved"
        }
    }

    func copyToPasteboard(_ text: String, _ what: String) {
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(text, forType: .string)
        notice = "Copied \(what)"
    }
    func copyCurl(_ e: Endpoint, _ what: String = "curl command") {
        guard client != nil, let url = URL(string: serverURL.trimmingCharacters(in: .whitespaces)) else { return }
        copyToPasteboard(Curl.script(e, server: url), what)
    }

    /// Runs one bulk action over the VMs it applies to, one at a time so a failure names the VM.
    /// Goes through the same role gate and confirmation as single actions.
    func bulk(_ action: BulkAction, on vms: [VmInfo]) {
        guard canWrite else { error = "\(action.title): your role (\(role ?? "unknown")) cannot make changes"; return }
        let plan = action.plan(vms)
        guard !plan.apply.isEmpty else { error = "\(action.title): none of the selected VMs can do that right now."; return }
        let names = plan.apply.map(\.name).joined(separator: ", ")
        let skipped = plan.skip.isEmpty ? "" : " Skipped (wrong state): \(plan.skip.map(\.name).joined(separator: ", "))."
        confirmAction("\(action.title) \(plan.apply.count) VM\(plan.apply.count == 1 ? "" : "s")?", message: names + "." + skipped,
                      label: action.title, destructive: action.destructive) { [self] in
            var failed: [String] = []
            let snap = BulkAction.snapshotName(at: Date())
            for vm in plan.apply {
                let ok = await perform("\(action.title) \(vm.name)") { c in
                    if let p = action.power { try await c.power(p, vm: vm) } else { try await c.createSnapshot(vm, name: snap, description: "Bulk snapshot from Machina for Mac") }
                }
                if !ok { failed.append(vm.name) }
            }
            if failed.isEmpty { notice = "\(action.title): \(plan.apply.count) done"; error = nil }
            else { error = "\(action.title) failed for \(failed.joined(separator: ", "))" }
            await refresh()
        }
    }

    // Saved servers (without passwords: those live in the Keychain, certificate pins too).
    var servers: [ServerProfile] = {
        (UserDefaults.standard.data(forKey: "servers")).flatMap { try? JSONDecoder().decode([ServerProfile].self, from: $0) } ?? []
    }()

    private func saveServers() { UserDefaults.standard.set(try? JSONEncoder().encode(servers), forKey: "servers") }

    func forget(_ p: ServerProfile) {
        servers = ServerList.removing(servers, id: p.id); saveServers()
        if let host = p.url.host { Keychain.remove("pw:\(p.username ?? "")@\(host)") }
    }

    func hasSavedPassword(_ p: ServerProfile) -> Bool { Keychain.get(passwordAccount(p)) != nil }
    private func passwordAccount(_ p: ServerProfile) -> String { "pw:\(p.username ?? "")@\(p.url.host ?? "")" }

    /// Switches to a saved server, signing in with its stored password; without one the login form is prefilled.
    func switchTo(_ p: ServerProfile) async {
        if phase == .ready { await signOut() }
        serverURL = p.url.absoluteString; username = p.username ?? ""
        if let pw = Keychain.get(passwordAccount(p)) { await signIn(password: pw) }
    }

    func open(_ link: DeepLink) {
        switch link {
        case .vm(let n): selection = .vm(n)
        case .page(let id): selection = FeatureRegistry.spec(id) == nil ? selection : .page(id)
        case .fleet: selection = .fleet
        case .tasks: selection = .tasks
        case .alerts: selection = .alerts
        }
        NSApp.activate(ignoringOtherApps: true)
    }

    // Dialog state shared by every screen; the dialogs themselves are attached once in MainWindow.
    var prompt: PromptRequest?
    var pendingAction: PendingAction?
    var notice: String?
    var pendingApprovals = 0
    private var seenApprovals: Set<String>?
    private let notifications = NotificationDelegate()

    func api() throws -> MachinaClient {
        guard let client else { throw MachinaError.unauthenticated }
        return client
    }

    /// Runs a write. Viewers (role readonly) are stopped here with a clear message instead of a server 403.
    @discardableResult
    func perform(_ label: String, _ op: @Sendable (MachinaClient) async throws -> Void) async -> Bool {
        guard canWrite else { error = "\(label): your role (\(role ?? "unknown")) cannot make changes"; return false }
        do { try await op(try api()); error = nil; return true }
        catch { self.error = "\(label): \(error.localizedDescription)"; return false }
    }

    func ask(_ title: String, message: String = "", submit: String, fields: [PromptField],
             run: @escaping @MainActor ([String: String]) async -> Void) {
        prompt = PromptRequest(title: title, message: message, submit: submit, fields: fields, run: run)
    }

    func confirmAction(_ title: String, message: String, label: String, destructive: Bool = true,
                       run: @escaping @MainActor () async -> Void) {
        pendingAction = PendingAction(title: title, message: message, label: label, destructive: destructive, run: run)
    }

    var serverURL: String = UserDefaults.standard.string(forKey: "serverURL") ?? "https://"
    var username: String = UserDefaults.standard.string(forKey: "username") ?? ""

    private(set) var client: MachinaClient?
    private var watchTask: Task<Void, Never>?
    private var trustContinuation: CheckedContinuation<Bool, Never>?

    var runningCount: Int { vms.filter(\.isRunning).count }

    func signIn(password: String) async {
        error = nil
        guard let url = URL(string: serverURL.trimmingCharacters(in: .whitespaces)), url.host != nil else {
            error = "Enter a full server URL, e.g. https://host:5092"; return
        }
        phase = .connecting
        let account = "pin:\(url.host ?? ""):\(url.port ?? 443)"
        let delegate = PinnedTLSDelegate(
            pinned: Keychain.get(account),
            confirmFirstUse: { [weak self] fp in await self?.askTrust(fp) ?? false },
            persist: { Keychain.set($0, for: account) })
        let c = MachinaClient(server: ServerProfile(name: url.host ?? "Server", url: url),
                              session: MachinaClient.makeSession(delegate: delegate))
        do {
            try await c.login(user: username, password: password)
            UserDefaults.standard.set(serverURL, forKey: "serverURL")
            UserDefaults.standard.set(username, forKey: "username")
            Keychain.set(password, for: "pw:\(username)@\(url.host ?? "")")
            servers = ServerList.remembering(servers, url: url, username: username); saveServers()
            client = c
            role = (try? await c.sessionInfo())?.role
            await refresh()
            phase = .ready
            startWatching(c)
            notifications.model = self
            let center = UNUserNotificationCenter.current()
            center.delegate = notifications
            // Review opens the app on the approvals page. Reject (the safe direction) needs the Mac to be unlocked;
            // approving stays in the app, behind a confirmation, because a banner does not show the plan.
            center.setNotificationCategories([UNNotificationCategory(identifier: "APPROVAL", actions: [
                UNNotificationAction(identifier: "REVIEW", title: "Review", options: [.foreground]),
                UNNotificationAction(identifier: "REJECT", title: "Reject", options: [.authenticationRequired, .destructive]),
            ], intentIdentifiers: [], options: [])])
            _ = try? await center.requestAuthorization(options: [.alert, .sound])
        } catch {
            self.error = error.localizedDescription
            phase = .signedOut
        }
    }

    func signOut() async {
        watchTask?.cancel(); watchTask = nil
        await client?.logout()
        SpotlightIndexer.shared.clear(); client = nil; seenApprovals = nil; pendingApprovals = 0; role = nil; vms = []; hosts = []; tasks = []; alerts = []; metrics = nil
        platformLoaded = false; selection = nil; live = false; phase = .signedOut
    }

    /// Polled every 30 s while signed in: updates the sidebar badge and notifies for requests that appeared since the last poll.
    func refreshApprovals() async {
        guard phase == .ready, let client, let pending = try? await client.pendingApprovals() else { return }
        pendingApprovals = pending.count
        let fresh = Approvals.newPending(seen: seenApprovals, current: pending.map { ($0.id, "pending") })
        seenApprovals = Set(pending.map(\.id))
        for id in fresh {
            let content = UNMutableNotificationContent()
            content.title = "Firewall change needs approval"
            content.body = pending.first { $0.id == id }?.summary ?? ""
            content.categoryIdentifier = "APPROVAL"
            content.userInfo = ["approvalId": id]
            try? await UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: "approval-\(id)", content: content, trigger: nil))
        }
    }

    func rejectApproval(_ id: String) async {
        if await perform("Reject approval", { _ = try await $0.call(Endpoint("POST", "/api/v1/zeus-firewall/approvals/\(MachinaClient.seg(id))/reject", body: .object([:]), controller: true)) }) {
            notice = "Rejected approval request"; await refreshApprovals()
        }
    }

    /// Generic read for registry pages (`ResourceListView`): plain call, derived transform or fan-out.
    func load(_ spec: ResourceSpec) async throws -> JSONValue {
        guard let client else { throw MachinaError.unauthenticated }
        return try await client.load(spec)
    }

    func call(_ endpoint: Endpoint) async throws -> JSONValue {
        guard let client else { throw MachinaError.unauthenticated }
        return try await client.call(endpoint)
    }

    func refresh() async {
        guard let client else { return }
        do { vms = try await client.listVMs() } catch { self.error = error.localizedDescription }
    }

    /// Loads hosts, tasks, metrics and alerts independently so one failing endpoint
    /// (e.g. a controller that is down) does not blank the others.
    func refreshPlatform() async {
        guard let client else { return }
        async let h = capture { try await client.hosts() }
        async let t = capture { try await client.tasks() }
        async let m = capture { try await client.localMetrics() }
        async let a = capture { try await client.alerts() }
        async let hi = capture { try await client.metricsHistory(limit: 120) }
        let (hr, tr, mr, ar, hir) = await (h, t, m, a, hi)
        if case .success(let v) = hir { history = v }
        var errors: [String] = []
        switch hr { case .success(let v): hosts = v; case .failure(let e): errors.append("hosts: \(e.localizedDescription)") }
        switch tr { case .success(let v): tasks = v; case .failure(let e): errors.append("tasks: \(e.localizedDescription)") }
        switch mr { case .success(let v): metrics = v; case .failure(let e): errors.append("metrics: \(e.localizedDescription)") }
        switch ar { case .success(let v): alerts = v.alerts; unackedAlerts = v.totalUnacknowledged
                    case .failure(let e): errors.append("alerts: \(e.localizedDescription)") }
        platformError = errors.isEmpty ? nil : errors.joined(separator: " · ")
        platformLoaded = true
    }

    func taskAction(cancel: Bool, _ task: PlatformTask) async {
        guard let client else { return }
        do {
            if cancel { try await client.cancelTask(task.id) } else { try await client.retryTask(task.id) }
            error = nil
        } catch { self.error = "\(cancel ? "Cancel" : "Retry") \(task.operation): \(error.localizedDescription)" }
        await refreshPlatform()
    }

    private func capture<T: Sendable>(_ body: @Sendable () async throws -> T) async -> Result<T, Error> {
        do { return .success(try await body()) } catch { return .failure(error) }
    }

    func power(_ action: PowerAction, _ vm: VmInfo) async {
        guard let client else { return }
        do { try await client.power(action, vm: vm); error = nil }
        catch { self.error = "\(action.rawValue) \(vm.name): \(error.localizedDescription)" }
    }

    // MARK: TLS first-use confirmation

    private func askTrust(_ fingerprint: String) async -> Bool {
        await withCheckedContinuation { cont in
            trustContinuation = cont
            pendingFingerprint = fingerprint
        }
    }

    func resolveTrust(_ accept: Bool) {
        pendingFingerprint = nil
        trustContinuation?.resume(returning: accept)
        trustContinuation = nil
    }

    // MARK: Live updates

    private func startWatching(_ c: MachinaClient) {
        watchTask?.cancel()
        watchTask = Task { [weak self] in
            for await ev in c.watch() {
                guard let self else { return }
                switch ev {
                case .heartbeat: self.live = true
                case .disconnected: self.live = false
                case .changes(let changes):
                    self.live = true
                    self.notify(changes)
                    await self.refresh()
                }
            }
        }
    }

    private func notify(_ changes: [VmChange]) {
        for ch in changes where ch.event == "state_change" {
            let content = UNMutableNotificationContent()
            content.title = ch.name
            content.body = "\(ch.oldState ?? "?") → \(ch.newState ?? "?")"
            UNUserNotificationCenter.current().add(
                UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
        }
    }
}

/// Handles taps and actions on approval notifications. Delegate callbacks arrive off the main actor.
final class NotificationDelegate: NSObject, UNUserNotificationCenterDelegate, @unchecked Sendable {
    @MainActor weak var model: AppModel?

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent n: UNNotification) async -> UNNotificationPresentationOptions { [.banner, .sound] }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        guard let id = response.notification.request.content.userInfo["approvalId"] as? String else { return }
        let action = response.actionIdentifier
        await MainActor.run {
            guard let model else { return }
            if action == "REJECT" { Task { await model.rejectApproval(id) } }
            else { model.selection = .page("approvals"); NSApp.activate(ignoringOtherApps: true) }
        }
    }
}
