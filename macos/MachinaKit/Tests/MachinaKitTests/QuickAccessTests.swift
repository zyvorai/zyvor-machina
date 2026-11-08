import Testing
import Foundation
@testable import MachinaKit

private var cal: Calendar { var c = Calendar(identifier: .gregorian); c.timeZone = TimeZone(identifier: "UTC")!; return c }
private func at(_ h: Int, _ m: Int = 0, day: Int = 10) -> Date { cal.date(from: DateComponents(year: 2026, month: 5, day: day, hour: h, minute: m))! }
private func iso(_ d: Date) -> String { ISO8601DateFormatter().string(from: d) }
private func al(_ sev: String, _ msg: String, _ when: Date, ack: Bool = false) -> FleetAlert {
    FleetAlert(id: UUID().uuidString, ruleName: "r", message: msg, severity: sev, timestamp: iso(when), acknowledged: ack, peer: nil)
}
private func task(_ status: String, _ when: Date, op: String = "vm.backup", msg: String? = nil) -> PlatformTask {
    PlatformTask(id: UUID().uuidString, operation: op, status: status, progress: nil, message: msg, createdAt: iso(when))
}

@Test func hotKeyValidationAndLabel() {
    #expect(HotKeySpec.default.isValid)
    #expect(HotKeySpec.default.label == "⌃⌥⌘K")
    #expect(!HotKeySpec(keyCode: 40, modifiers: 0).isValid)
    #expect(!HotKeySpec(keyCode: 40, modifiers: HotKeySpec.shift).isValid)
    #expect(!HotKeySpec(keyCode: 400, modifiers: HotKeySpec.cmd).isValid)
    #expect(!HotKeySpec(keyCode: 40, modifiers: HotKeySpec.cmd | 1).isValid)
    #expect(HotKeySpec.presets.allSatisfy { $0.isValid })
    let data = try! JSONEncoder().encode(HotKeySpec.presets[1])
    #expect(try! JSONDecoder().decode(HotKeySpec.self, from: data) == HotKeySpec.presets[1])
    #expect(HotKeySpec.presets[1].label == "⌥⌘Space")
}

@Test func digestDefaultsOffAndClamps() {
    #expect(DigestSettings().enabled == false)
    #expect(DigestSettings(minuteOfDay: 5000, lookbackHours: 0).minuteOfDay == 1439)
    #expect(DigestSettings(lookbackHours: 999).lookbackHours == 72)
}

@Test func digestCountsOnlyRecentUnacknowledgedAndFailed() {
    let since = at(18, day: 9)
    let d = DigestBuilder.build(
        alerts: [al("critical", "disk full on web1", at(2)), al("warning", "old", at(9, day: 8)), al("critical", "acked", at(3), ack: true)],
        tasks: [task("failed", at(1)), task("completed", at(1)), task("failed", at(10, day: 8))], since: since)
    #expect(d.alertCount == 1 && d.failedTaskCount == 1)
    #expect(d.worst == .critical)
    #expect(d.title == "1 alert (1 critical), 1 failed task")
    #expect(d.body.contains("disk full on web1") && d.body.contains("vm.backup"))
    #expect(d.link == .alerts)
}

@Test func digestAllQuietAndTaskOnly() {
    let q = DigestBuilder.build(alerts: [], tasks: [], since: at(0))
    #expect(q.isEmpty && q.worst == .ok && q.title == "All quiet overnight")
    let t = DigestBuilder.build(alerts: [], tasks: [task("error", at(3))], since: at(0))
    #expect(t.worst == .warning && t.link == .tasks)
}

@Test func digestKeepsUnparseableTimestamps() {
    let a = FleetAlert(id: "1", ruleName: "r", message: "m", severity: "info", timestamp: "garbage", acknowledged: false, peer: nil)
    #expect(DigestBuilder.build(alerts: [a], tasks: [], since: at(0)).alertCount == 1)
}

@Test func nextFireIsStrictlyAfter() {
    #expect(DigestBuilder.nextFire(after: at(7), minuteOfDay: 480, calendar: cal) == at(8))
    #expect(DigestBuilder.nextFire(after: at(8), minuteOfDay: 480, calendar: cal) == at(8, day: 11))
    #expect(DigestBuilder.nextFire(after: at(9), minuteOfDay: 480, calendar: cal) == at(8, day: 11))
}

@Test func deliveryRespectsQuietHours() {
    let quiet = QuietHours(enabled: true, startMinute: 22 * 60, endMinute: 9 * 60, breakThrough: "critical")
    let warn = DigestBuilder.build(alerts: [al("warning", "x", at(3))], tasks: [], since: at(0))
    let crit = DigestBuilder.build(alerts: [al("critical", "x", at(3))], tasks: [], since: at(0))
    #expect(DigestBuilder.delivery(for: warn, quiet: quiet, at: at(8), calendar: cal) == .deferUntil(at(9)))
    #expect(DigestBuilder.delivery(for: crit, quiet: quiet, at: at(8), calendar: cal) == .now)
    #expect(DigestBuilder.delivery(for: warn, quiet: quiet, at: at(10), calendar: cal) == .now)
    #expect(DigestBuilder.delivery(for: warn, quiet: QuietHours(), at: at(8), calendar: cal) == .now)
}

@Test func digestNotPostedTwice() {
    #expect(!DigestBuilder.alreadyPosted(lastPosted: nil, due: at(8), calendar: cal))
    #expect(DigestBuilder.alreadyPosted(lastPosted: at(8, 1), due: at(8), calendar: cal))
    #expect(!DigestBuilder.alreadyPosted(lastPosted: at(8, day: 9), due: at(8), calendar: cal))
}

@Test func shareLinkCarriesNoSecrets() {
    let server = URL(string: "https://Admin:pw@Host.example:5092/api?token=abc")!
    let u = ShareLink.url(for: .vm("web 1"), server: server)
    #expect(!u.absoluteString.contains("pw") && !u.absoluteString.contains("token") && !u.absoluteString.contains("Admin"))
    #expect(ShareLink.serverHint(in: u) == "https://host.example:5092")
    #expect(DeepLink(url: u) == .vm("web 1"))
    #expect(ShareLink.url(for: .fleet, server: nil).absoluteString == "machina://fleet")
    #expect(ShareLink.hint(from: URL(string: "ftp://h")!) == nil)
}

@Test func shareLinkMatchesServer() {
    let u = ShareLink.url(for: .vm("a"), server: URL(string: "https://h:5092")!)
    #expect(ShareLink.match(link: u, current: URL(string: "https://H:5092/x")) == .same)
    #expect(ShareLink.match(link: u, current: URL(string: "https://other:5092")) == .different("https://h:5092"))
    #expect(ShareLink.match(link: URL(string: "machina://vm/a")!, current: nil) == .noHint)
}

@Test func handoffActivityRoundTrip() {
    let info = ShareLink.activityInfo(vm: "db", server: URL(string: "https://h:5092"))
    let r = ShareLink.link(fromActivity: info)
    #expect(r?.link == .vm("db") && r?.server == "https://h:5092")
    #expect(ShareLink.link(fromActivity: ["vm": ""]) == nil)
    #expect(ShareLink.link(fromActivity: nil) == nil)
}

@Test func glanceSummarisesOneVm() {
    let vm = VmInfo(name: "web1", state: "running", vcpus: 4, memoryMb: 4096, guestIp: "10.0.0.5")
    let g = VmGlance.make(vm: vm, alerts: [al("critical", "web1 cpu", at(1)), al("critical", "other", at(1)), al("warning", "web1 x", at(1), ack: true)],
                          tasks: [task("running", at(1), op: "vm.backup", msg: "web1"), task("running", at(1), msg: "db")])
    #expect(g.stateLine == "running · 4 vCPU · 4 GiB")
    #expect(g.level == .critical)
    #expect(g.notes == ["critical: web1 cpu", "vm.backup running", "IP 10.0.0.5"])
    let off = VmGlance.make(vm: VmInfo(name: "x", state: "shutoff", vcpus: 1, memoryMb: 512), alerts: [], tasks: [])
    #expect(off.level == .ok && off.stateLine == "shutoff · 1 vCPU · 512 MiB")
}
