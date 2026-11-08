import Foundation

/// Pure logic for the "reach Machina from anywhere" features: the global hotkey, the morning digest,
/// copy/Handoff links and the floating VM glance. No UI, no network.

// MARK: Global hotkey

/// A global shortcut as Carbon stores it. Modifier bits are Carbon's (`cmdKey` 256, `shiftKey` 512, `optionKey` 2048, `controlKey` 4096).
public struct HotKeySpec: Equatable, Hashable, Codable, Sendable {
    public static let cmd: UInt32 = 256, shift: UInt32 = 512, option: UInt32 = 2048, control: UInt32 = 4096
    public var keyCode: UInt32
    public var modifiers: UInt32

    public init(keyCode: UInt32, modifiers: UInt32) { self.keyCode = keyCode; self.modifiers = modifiers }

    /// Control-Option-Command-K: unlikely to clash with another app, and never typed by accident.
    public static let `default` = HotKeySpec(keyCode: 40, modifiers: cmd | option | control)

    /// A global key needs Command, Control or Option; Shift alone (or nothing) would swallow normal typing in every app.
    public var isValid: Bool { modifiers & (Self.cmd | Self.control | Self.option) != 0 && keyCode < 128 && modifiers & ~(Self.cmd | Self.shift | Self.option | Self.control) == 0 }

    public static let keyNames: [UInt32: String] = [
        0: "A", 1: "S", 2: "D", 3: "F", 4: "H", 5: "G", 6: "Z", 7: "X", 8: "C", 9: "V", 11: "B", 12: "Q", 13: "W", 14: "E", 15: "R",
        16: "Y", 17: "T", 31: "O", 32: "U", 34: "I", 35: "P", 37: "L", 38: "J", 40: "K", 45: "N", 46: "M", 49: "Space"]

    /// "⌃⌥⌘K" style label.
    public var label: String {
        var s = ""
        if modifiers & Self.control != 0 { s += "⌃" }
        if modifiers & Self.option != 0 { s += "⌥" }
        if modifiers & Self.shift != 0 { s += "⇧" }
        if modifiers & Self.cmd != 0 { s += "⌘" }
        return s + (Self.keyNames[keyCode] ?? "key \(keyCode)")
    }

    /// The choices offered in Settings (a recorder would need more UI than it is worth).
    public static let presets: [HotKeySpec] = [
        .default,
        HotKeySpec(keyCode: 49, modifiers: cmd | option),              // ⌥⌘Space
        HotKeySpec(keyCode: 49, modifiers: control | option),          // ⌃⌥Space
        HotKeySpec(keyCode: 46, modifiers: cmd | option | control),    // ⌃⌥⌘M
    ]
}

// MARK: Morning digest

public struct DigestSettings: Equatable, Codable, Sendable {
    public var enabled: Bool
    public var minuteOfDay: Int   // when to post, minutes after midnight
    public var lookbackHours: Int // how far back "overnight" reaches
    public init(enabled: Bool = false, minuteOfDay: Int = 8 * 60, lookbackHours: Int = 14) {
        self.enabled = enabled; self.minuteOfDay = min(max(minuteOfDay, 0), 1439); self.lookbackHours = min(max(lookbackHours, 1), 72)
    }
}

public struct HealthDigest: Equatable, Sendable {
    public var title: String
    public var body: String
    public var worst: HealthLevel
    public var alertCount: Int
    public var failedTaskCount: Int
    public var link: DeepLink
    public var isEmpty: Bool { alertCount == 0 && failedTaskCount == 0 }
}

public enum DigestBuilder {
    /// Alerts raised since `since` (acknowledged ones are left out) and failed tasks created since then.
    /// An alert or task whose timestamp cannot be parsed is kept: better one extra line than a missed failure.
    public static func build(alerts: [FleetAlert], tasks: [PlatformTask], since: Date) -> HealthDigest {
        let newAlerts = alerts.filter { !$0.acknowledged && (VmDoctor.parseDate($0.timestamp).map { $0 >= since } ?? true) }
        let failed = tasks.filter { $0.isFailed && (VmDoctor.parseDate($0.createdAt).map { $0 >= since } ?? true) }
        let worstRank = newAlerts.map { QuietHours.rank($0.severity) }.max() ?? 0
        let worst: HealthLevel = worstRank >= 3 ? .critical : (worstRank == 2 || !failed.isEmpty) ? .warning : .ok
        var parts: [String] = []
        if !newAlerts.isEmpty {
            let crit = newAlerts.filter { QuietHours.rank($0.severity) >= 3 }.count
            parts.append("\(newAlerts.count) alert\(newAlerts.count == 1 ? "" : "s")" + (crit > 0 ? " (\(crit) critical)" : ""))
        }
        if !failed.isEmpty { parts.append("\(failed.count) failed task\(failed.count == 1 ? "" : "s")") }
        if parts.isEmpty {
            return HealthDigest(title: "All quiet overnight", body: "No new alerts or failed tasks.", worst: .ok, alertCount: 0, failedTaskCount: 0, link: .fleet)
        }
        let top = newAlerts.sorted { QuietHours.rank($0.severity) > QuietHours.rank($1.severity) }.prefix(2).map(\.message)
        let ops = failed.prefix(2).map(\.operation)
        let detail = (top + ops).joined(separator: " · ")
        return HealthDigest(title: parts.joined(separator: ", "), body: detail, worst: worst,
                            alertCount: newAlerts.count, failedTaskCount: failed.count, link: newAlerts.isEmpty ? .tasks : .alerts)
    }

    /// The next time `minuteOfDay` occurs strictly after `date`.
    public static func nextFire(after date: Date, minuteOfDay: Int, calendar: Calendar = .current) -> Date {
        var c = calendar.dateComponents([.year, .month, .day], from: date)
        c.hour = minuteOfDay / 60; c.minute = minuteOfDay % 60; c.second = 0
        let today = calendar.date(from: c) ?? date
        return today > date ? today : (calendar.date(byAdding: .day, value: 1, to: today) ?? date.addingTimeInterval(86400))
    }

    /// A digest posts at the chosen time unless quiet hours cover that time and nothing in it reaches the break-through
    /// severity; then it waits for quiet hours to end (`deferUntil`). An empty digest still posts: silence would look like a bug.
    public enum Delivery: Equatable, Sendable { case now, deferUntil(Date) }
    public static func delivery(for digest: HealthDigest, quiet: QuietHours, at date: Date, calendar: Calendar = .current) -> Delivery {
        let severity = digest.worst == .critical ? "critical" : digest.worst == .warning ? "warning" : "info"
        guard !quiet.allows(severity: severity, at: date, calendar: calendar) else { return .now }
        return .deferUntil(nextFire(after: date, minuteOfDay: quiet.endMinute, calendar: calendar))
    }

    /// Whether a digest due at `due` was already posted (the app may be relaunched near the deadline).
    public static func alreadyPosted(lastPosted: Date?, due: Date, calendar: Calendar = .current) -> Bool {
        guard let lastPosted else { return false }
        return calendar.isDate(lastPosted, inSameDayAs: due) && lastPosted >= due.addingTimeInterval(-60)
    }
}

// MARK: Share link and Handoff

/// `machina://vm/<name>?server=<scheme://host:port>`. The server hint carries no user, password or token, so a link can be pasted
/// into chat or handed to another Mac without leaking anything; the receiving app uses it only to warn when it is signed in elsewhere.
public enum ShareLink {
    public static let activityType = "dev.zyvor.machina.vm"

    public static func url(for link: DeepLink, server: URL?) -> URL {
        var c = URLComponents(url: link.url, resolvingAgainstBaseURL: false)!
        if let server, let hint = hint(from: server) { c.queryItems = [URLQueryItem(name: "server", value: hint)] }
        return c.url!
    }

    /// scheme://host[:port], lowercased, never user info, path or query.
    public static func hint(from url: URL) -> String? {
        guard let s = url.scheme?.lowercased(), let h = url.host?.lowercased(), s == "https" || s == "http" else { return nil }
        return url.port.map { "\(s)://\(h):\($0)" } ?? "\(s)://\(h)"
    }

    public static func serverHint(in url: URL) -> String? {
        URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "server" }?.value.flatMap { URL(string: $0) }.flatMap(hint(from:))
    }

    public enum Match: Equatable, Sendable { case noHint, same, different(String) }
    public static func match(link: URL, current: URL?) -> Match {
        guard let want = serverHint(in: link) else { return .noHint }
        return current.flatMap(hint(from:)) == want ? .same : .different(want)
    }

    public static func link(fromActivity info: [AnyHashable: Any]?) -> (link: DeepLink, server: String?)? {
        guard let name = info?["vm"] as? String, !name.isEmpty else { return nil }
        return (.vm(name), info?["server"] as? String)
    }

    public static func activityInfo(vm: String, server: URL?) -> [String: String] {
        var i = ["vm": vm]
        if let server, let h = hint(from: server) { i["server"] = h }
        return i
    }
}

// MARK: Floating glance

/// What the always-on-top VM panel shows: state, size, and anything open about this VM. Read-only on purpose.
public struct VmGlance: Equatable, Sendable {
    public var name: String
    public var stateLine: String
    public var level: HealthLevel
    public var notes: [String]

    public static func make(vm: VmInfo, alerts: [FleetAlert], tasks: [PlatformTask]) -> VmGlance {
        let mine = alerts.filter { !$0.acknowledged && ($0.message.contains(vm.name) || $0.ruleName.contains(vm.name)) }
        let active = tasks.filter { $0.isActive && ($0.message?.contains(vm.name) ?? false) }
        var notes: [String] = mine.prefix(3).map { "\($0.severity): \($0.message)" }
        notes += active.prefix(2).map { "\($0.operation) \($0.status)" }
        if let ip = vm.guestIp ?? vm.guestIps?.first { notes.append("IP \(ip)") }
        let worst = mine.map { QuietHours.rank($0.severity) }.max() ?? 0
        let level: HealthLevel = worst >= 3 ? .critical : (worst == 2 || (!vm.isRunning && !["shutoff", "stopped", "paused"].contains(vm.state.lowercased()))) ? .warning : .ok
        return VmGlance(name: vm.name, stateLine: "\(vm.state) · \(vm.vcpus) vCPU · \(vm.memoryMb >= 1024 ? "\(vm.memoryMb / 1024) GiB" : "\(vm.memoryMb) MiB")", level: level, notes: notes)
    }
}
