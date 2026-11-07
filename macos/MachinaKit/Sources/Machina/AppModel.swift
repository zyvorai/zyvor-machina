import Foundation
import Observation
import MachinaKit
import UserNotifications

enum SidebarItem: Hashable { case fleet, tasks, alerts, vm(String) }

@MainActor @Observable
final class AppModel {
    enum Phase { case signedOut, connecting, ready }

    var phase: Phase = .signedOut
    var vms: [VmInfo] = []
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
    var platformError: String?
    var platformLoaded = false

    var serverURL: String = UserDefaults.standard.string(forKey: "serverURL") ?? "https://"
    var username: String = UserDefaults.standard.string(forKey: "username") ?? ""

    private var client: MachinaClient?
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
            client = c
            await refresh()
            phase = .ready
            startWatching(c)
            _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound])
        } catch {
            self.error = error.localizedDescription
            phase = .signedOut
        }
    }

    func signOut() async {
        watchTask?.cancel(); watchTask = nil
        await client?.logout()
        client = nil; vms = []; hosts = []; tasks = []; alerts = []; metrics = nil
        platformLoaded = false; selection = nil; live = false; phase = .signedOut
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
        let (hr, tr, mr, ar) = await (h, t, m, a)
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
