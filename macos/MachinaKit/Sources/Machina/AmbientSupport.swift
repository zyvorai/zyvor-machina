import SwiftUI
import Observation
import LocalAuthentication
import MachinaKit

/// Touch ID / Apple Watch / Mac password prompt. Falls back to "allowed" only when the Mac has no
/// authentication at all configured (nothing to ask), never when the user cancels or fails.
enum Biometric {
    @MainActor static func authenticate(_ reason: String) async -> Bool {
        let ctx = LAContext()
        var err: NSError?
        guard ctx.canEvaluatePolicy(.deviceOwnerAuthentication, error: &err) else {
            return err?.code == LAError.passcodeNotSet.rawValue
        }
        return (try? await ctx.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason)) ?? false
    }
}

/// User preferences, persisted in UserDefaults. One instance for the whole process.
@MainActor @Observable
final class AppSettings {
    static let shared = AppSettings()
    private let d = UserDefaults.standard

    var refreshSeconds: Int { didSet { d.set(refreshSeconds, forKey: "refreshSeconds") } }
    var notifyStateChanges: Bool { didSet { d.set(notifyStateChanges, forKey: "notifyStateChanges") } }
    var notifyAlerts: Bool { didSet { d.set(notifyAlerts, forKey: "notifyAlerts") } }
    var notifyApprovals: Bool { didSet { d.set(notifyApprovals, forKey: "notifyApprovals") } }
    var quietHours: QuietHours { didSet { d.set(try? JSONEncoder().encode(quietHours), forKey: "quietHours") } }
    var biometric: BiometricPolicy { didSet { d.set(try? JSONEncoder().encode(biometric), forKey: "biometric") } }
    var recentVMs: [String] { didSet { d.set(recentVMs, forKey: "recentVMs") } }

    private init() {
        func flag(_ k: String) -> Bool { UserDefaults.standard.object(forKey: k) as? Bool ?? true }
        let r = UserDefaults.standard.integer(forKey: "refreshSeconds")
        refreshSeconds = r > 0 ? min(max(r, 10), 600) : 30
        notifyStateChanges = flag("notifyStateChanges"); notifyAlerts = flag("notifyAlerts"); notifyApprovals = flag("notifyApprovals")
        quietHours = UserDefaults.standard.data(forKey: "quietHours").flatMap { try? JSONDecoder().decode(QuietHours.self, from: $0) } ?? QuietHours()
        biometric = UserDefaults.standard.data(forKey: "biometric").flatMap { try? JSONDecoder().decode(BiometricPolicy.self, from: $0) } ?? BiometricPolicy()
        recentVMs = UserDefaults.standard.stringArray(forKey: "recentVMs") ?? []
    }

    /// Whether a notification of this severity may be shown right now (quiet hours + the system Focus is handled by macOS itself).
    func mayNotify(severity: String) -> Bool { quietHours.allows(severity: severity, at: Date()) }
}

// MARK: Settings window

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var s = AppSettings.shared
    @State private var certs: [TrustedCert] = []

    var body: some View {
        TabView {
            Form {
                Picker("Refresh every", selection: $s.refreshSeconds) {
                    ForEach([10, 30, 60, 120, 300], id: \.self) { Text($0 < 60 ? "\($0) s" : "\($0 / 60) min").tag($0) }
                }
                Text("How often fleet health, alerts and approvals are polled while you are signed in.").font(.caption).foregroundStyle(.secondary)
            }.tabItem { Label("General", systemImage: "gearshape") }

            Form {
                Toggle("VM state changes", isOn: $s.notifyStateChanges)
                Toggle("New alerts", isOn: $s.notifyAlerts)
                Toggle("Firewall approvals", isOn: $s.notifyApprovals)
                Section("Quiet hours") {
                    Toggle("Hold back lower-severity notifications", isOn: $s.quietHours.enabled)
                    HStack {
                        minutePicker("From", $s.quietHours.startMinute)
                        minutePicker("To", $s.quietHours.endMinute)
                    }.disabled(!s.quietHours.enabled)
                    Picker("Still allow", selection: $s.quietHours.breakThrough) {
                        Text("Critical only").tag("critical"); Text("Warning and above").tag("warning"); Text("Everything").tag("info")
                    }.disabled(!s.quietHours.enabled)
                    Text("macOS Focus modes apply on top of this: a Focus that silences Machina silences these too.").font(.caption).foregroundStyle(.secondary)
                }
            }.tabItem { Label("Notifications", systemImage: "bell") }

            Form {
                Toggle("Approving a firewall change from a notification", isOn: $s.biometric.approvals)
                Toggle("Destructive actions (force stop, delete, revert, restore)", isOn: $s.biometric.destructiveActions)
                Toggle("Signing in with a stored password (switching servers)", isOn: $s.biometric.storedPasswords)
                Text("Asks for Touch ID, Apple Watch or your Mac password first.").font(.caption).foregroundStyle(.secondary)
            }.tabItem { Label("Security", systemImage: "touchid") }

            Form {
                if certs.isEmpty { Text("No pinned certificates.").foregroundStyle(.secondary) }
                ForEach(certs) { c in
                    HStack {
                        Text("\(c.host):\(c.port)")
                        Spacer()
                        Button("Revoke", role: .destructive) { Keychain.remove(c.account); reload() }
                    }
                }
                Text("Revoking forgets the pin; the next connection asks you to trust the host's certificate again.").font(.caption).foregroundStyle(.secondary)
            }.tabItem { Label("Trusted certificates", systemImage: "checkmark.shield") }

            QuickAccessSettingsForm().tabItem { Label("Quick access", systemImage: "bolt") }
            AIOperatorSettingsForm().tabItem { Label("AI operator", systemImage: "sparkles") }
        }
        .formStyle(.grouped).frame(width: 520, height: 440).onAppear(perform: reload)
    }

    private func reload() { certs = Keychain.accounts().compactMap(TrustedCert.parse).sorted { $0.host < $1.host } }

    private func minutePicker(_ title: String, _ value: Binding<Int>) -> some View {
        Picker(title, selection: value) {
            ForEach(Array(stride(from: 0, to: 24 * 60, by: 30)), id: \.self) { Text(QuietHours.label($0)).tag($0) }
        }
    }
}

// MARK: VM doctor

struct VMDoctorTab: View {
    @Environment(AppModel.self) private var model
    let vm: VmInfo
    @State private var report: VmDoctorReport?
    @State private var failed: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Doctor").font(.title2.weight(.semibold))
                if let r = report { Image(systemName: icon(r.level)).foregroundStyle(color(r.level)) }
                Spacer()
                Button { Task { await load() } } label: { Image(systemName: "arrow.clockwise") }
            }
            if let r = report {
                HStack(spacing: 28) {
                    stat("Last backup", r.lastBackup.map { VmDoctor.age($0, now: Date()) } ?? "never")
                    stat("Snapshots", "\(r.snapshotCount)")
                    stat("Open alerts", "\(r.openAlerts)")
                    stat("State", vm.state)
                }
                if r.findings.isEmpty { Label("Nothing to flag.", systemImage: "checkmark.circle").foregroundStyle(.green) }
                ForEach(r.findings) { f in
                    Label { VStack(alignment: .leading) { Text(f.title).font(.headline); Text(f.detail).foregroundStyle(.secondary) } }
                    icon: { Image(systemName: icon(f.level)).foregroundStyle(color(f.level)) }
                }
            } else if let failed { Text(failed).foregroundStyle(.red) } else { ProgressView() }
            Spacer()
        }.padding(24).frame(maxWidth: .infinity, alignment: .topLeading).task(id: vm.name) { await load() }
    }

    private func stat(_ t: String, _ v: String) -> some View {
        VStack(alignment: .leading) { Text(t).font(.caption).foregroundStyle(.secondary); Text(v).font(.title3) }
    }
    private func icon(_ l: HealthLevel) -> String { l == .ok ? "checkmark.circle.fill" : l == .warning ? "exclamationmark.circle.fill" : "xmark.octagon.fill" }
    private func color(_ l: HealthLevel) -> Color { l == .ok ? .green : l == .warning ? .orange : .red }

    private func load() async {
        do {
            let c = try model.api()
            async let b = c.backups()
            async let s = c.snapshots(vm)
            let (backups, snaps) = (try await b, (try? await s) ?? [])
            report = VmDoctor.assess(vm: vm, backups: backups, snapshots: snaps, alerts: model.alerts)
            failed = nil
        } catch { failed = error.localizedDescription }
    }
}
