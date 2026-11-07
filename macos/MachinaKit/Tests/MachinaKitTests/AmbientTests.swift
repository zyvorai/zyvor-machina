import Testing
import Foundation
@testable import MachinaKit

private func alert(_ sev: String, _ msg: String = "x", ack: Bool = false, rule: String = "r") -> FleetAlert {
    FleetAlert(id: UUID().uuidString, ruleName: rule, message: msg, severity: sev, timestamp: "", acknowledged: ack, peer: nil)
}
private func host(_ state: String, maint: Bool? = nil) -> PlatformHost {
    PlatformHost(id: UUID().uuidString, hostname: "h", state: state, maintenanceMode: maint)
}
private func vm(_ name: String = "web1", state: String = "running") -> VmInfo {
    VmInfo(name: name, state: state, vcpus: 1, memoryMb: 512)
}
private func backup(_ vm: String, _ ts: String, _ status: String = "completed") -> BackupInfo {
    BackupInfo(id: UUID().uuidString, timestamp: ts, vmFilter: vm, status: status, withDisks: true)
}
private let now = Date(timeIntervalSince1970: 1_800_000_000)
private func iso(daysAgo: Double) -> String { ISO8601DateFormatter().string(from: now.addingTimeInterval(-daysAgo * 86400)) }

@Test func healthAllClearAndWarning() {
    #expect(FleetHealth.evaluate(alerts: [], hosts: [host("online")], pendingApprovals: 0).level == .ok)
    let w = FleetHealth.evaluate(alerts: [alert("warning"), alert("critical", ack: true)], hosts: [], pendingApprovals: 2)
    #expect(w.level == .warning)
    #expect(w.summary == "1 open alert, 2 approvals pending")
}

@Test func healthCriticalOnOfflineHostButNotMaintenance() {
    #expect(FleetHealth.evaluate(alerts: [], hosts: [host("offline")], pendingApprovals: 0).level == .critical)
    #expect(FleetHealth.evaluate(alerts: [], hosts: [host("offline", maint: true)], pendingApprovals: 0).level == .ok)
    let c = FleetHealth.evaluate(alerts: [alert("critical"), alert("info")], hosts: [], pendingApprovals: 0)
    #expect(c.level == .critical && c.summary == "1 critical alert, 1 other alert")
    #expect(FleetHealth.evaluate(alerts: [], hosts: [], pendingApprovals: 0, connected: false).summary == "Not connected")
}

private var utc: Calendar { var c = Calendar(identifier: .gregorian); c.timeZone = TimeZone(identifier: "UTC")!; return c }
private func at(_ h: Int, _ m: Int = 0) -> Date {
    utc.date(from: DateComponents(year: 2026, month: 1, day: 5, hour: h, minute: m))!
}

@Test func quietHoursWrapMidnight() {
    let q = QuietHours(enabled: true, startMinute: 22 * 60, endMinute: 7 * 60)
    #expect(q.isQuiet(at: at(23), calendar: utc))
    #expect(q.isQuiet(at: at(3), calendar: utc))
    #expect(!q.isQuiet(at: at(7), calendar: utc))
    #expect(!q.isQuiet(at: at(12), calendar: utc))
    #expect(q.isQuiet(at: at(22), calendar: utc))
}

@Test func quietHoursSameDayAndDisabled() {
    let q = QuietHours(enabled: true, startMinute: 9 * 60, endMinute: 17 * 60)
    #expect(q.isQuiet(at: at(10), calendar: utc) && !q.isQuiet(at: at(18), calendar: utc))
    #expect(!QuietHours(enabled: false).isQuiet(at: at(23), calendar: utc))
    #expect(!QuietHours(enabled: true, startMinute: 60, endMinute: 60).isQuiet(at: at(1), calendar: utc))
}

@Test func quietHoursLetCriticalThrough() {
    let q = QuietHours(enabled: true, startMinute: 22 * 60, endMinute: 7 * 60, breakThrough: "critical")
    #expect(!q.allows(severity: "warning", at: at(2), calendar: utc))
    #expect(q.allows(severity: "critical", at: at(2), calendar: utc))
    #expect(q.allows(severity: "info", at: at(12), calendar: utc))
    #expect(QuietHours.label(7 * 60 + 5) == "07:05")
}

@Test func parsesDates() {
    #expect(VmDoctor.parseDate("2026-01-05T10:00:00Z") != nil)
    #expect(VmDoctor.parseDate("2026-01-05T10:00:00.123Z") != nil)
    #expect(VmDoctor.parseDate("2026-01-05 10:00:00") != nil)
    #expect(VmDoctor.parseDate("1800000000")?.timeIntervalSince1970 == 1_800_000_000)
    #expect(VmDoctor.parseDate("1800000000000")?.timeIntervalSince1970 == 1_800_000_000)
    #expect(VmDoctor.parseDate("garbage") == nil && VmDoctor.parseDate(nil) == nil)
}

@Test func doctorBackupAging() {
    let fresh = VmDoctor.assess(vm: vm(), backups: [backup("web1", iso(daysAgo: 1))], snapshots: [], alerts: [], now: now)
    #expect(fresh.level == .ok && fresh.lastBackup != nil)
    let old = VmDoctor.assess(vm: vm(), backups: [backup("web1", iso(daysAgo: 9))], snapshots: [], alerts: [], now: now)
    #expect(old.level == .warning)
    let stale = VmDoctor.assess(vm: vm(), backups: [backup("web1", iso(daysAgo: 20))], snapshots: [], alerts: [], now: now)
    #expect(stale.level == .critical)
}

@Test func doctorIgnoresFailedAndOtherVmBackups() {
    let r = VmDoctor.assess(vm: vm(), backups: [backup("web1", iso(daysAgo: 1), "failed"), backup("db", iso(daysAgo: 1))], snapshots: [], alerts: [], now: now)
    #expect(r.level == .critical && r.lastBackup == nil)
    let none = VmDoctor.assess(vm: vm(), backups: [], snapshots: [], alerts: [], now: now)
    #expect(none.findings.first?.title == "No backups" && none.level == .warning)
}

@Test func doctorAlertsAndSnapshots() {
    let snaps = (0..<11).map { VmSnapshot(name: "s\($0)") }
    let r = VmDoctor.assess(vm: vm(), backups: [backup("web1", iso(daysAgo: 0.1))], snapshots: snaps,
                            alerts: [alert("critical", "web1 disk full"), alert("critical", "db1 down"), alert("critical", "web1 old", ack: true)], now: now)
    #expect(r.snapshotCount == 11 && r.openAlerts == 1 && r.level == .critical)
    #expect(r.findings.map(\.title).contains("Many snapshots"))
}

@Test func recentVMsAreMRUUniqueAndCapped() {
    var l: [String] = []
    for n in ["a", "b", "c", "a", "d", "e", "f"] { l = RecentVMs.touching(l, n, limit: 5) }
    #expect(l == ["f", "e", "d", "a", "c"])
    #expect(RecentVMs.existing(["x", "a"], in: [vm("a")]).map(\.name) == ["a"])
}

@Test func biometricPolicyAndTrustedCerts() {
    #expect(!BiometricPolicy().required(destructive: true))
    #expect(BiometricPolicy(destructiveActions: true).required(destructive: true))
    #expect(!BiometricPolicy(destructiveActions: true).required(destructive: false))
    #expect(TrustedCert.parse(account: "pin:host.example:5092")?.port == 5092)
    #expect(TrustedCert.parse(account: "pw:sus@host") == nil)
    #expect(TrustedCert.parse(account: "pin::5092") == nil)
}
