import Foundation

/// Pure logic behind the Mac-native ambient features: menu bar health glyph, quiet hours,
/// the per-VM doctor and the recent-VMs list. No UI and no network here, so all of it is unit-tested.

public enum HealthLevel: Int, Comparable, Sendable {
    case ok = 0, warning, critical
    public static func < (a: Self, b: Self) -> Bool { a.rawValue < b.rawValue }
}

public struct FleetHealth: Equatable, Sendable {
    public var level: HealthLevel
    public var symbol: String      // SF Symbol for the menu bar
    public var summary: String

    /// Critical: a critical unacknowledged alert or an offline host. Warning: any other unacknowledged alert,
    /// a pending approval, or not connected. Otherwise ok.
    public static func evaluate(alerts: [FleetAlert], hosts: [PlatformHost], pendingApprovals: Int, connected: Bool = true) -> FleetHealth {
        guard connected else { return .init(level: .warning, symbol: "server.rack", summary: "Not connected") }
        let open = alerts.filter { !$0.acknowledged }
        let critical = open.filter { QuietHours.rank($0.severity) >= 3 }.count
        let offline = hosts.filter { !$0.isOnline && $0.maintenanceMode != true }.count
        var parts: [String] = []
        if critical > 0 { parts.append("\(critical) critical alert\(critical == 1 ? "" : "s")") }
        if offline > 0 { parts.append("\(offline) host\(offline == 1 ? "" : "s") offline") }
        if critical > 0 || offline > 0 {
            let rest = open.count - critical
            if rest > 0 { parts.append("\(rest) other alert\(rest == 1 ? "" : "s")") }
            if pendingApprovals > 0 { parts.append("\(pendingApprovals) approval\(pendingApprovals == 1 ? "" : "s") pending") }
            return .init(level: .critical, symbol: "exclamationmark.triangle.fill", summary: parts.joined(separator: ", "))
        }
        if !open.isEmpty { parts.append("\(open.count) open alert\(open.count == 1 ? "" : "s")") }
        if pendingApprovals > 0 { parts.append("\(pendingApprovals) approval\(pendingApprovals == 1 ? "" : "s") pending") }
        if !parts.isEmpty { return .init(level: .warning, symbol: "exclamationmark.circle", summary: parts.joined(separator: ", ")) }
        return .init(level: .ok, symbol: "server.rack", summary: "All clear")
    }
}

/// Quiet hours: between `startMinute` and `endMinute` (minutes after midnight, may wrap past midnight)
/// notifications below `breakThrough` severity are held back.
public struct QuietHours: Equatable, Codable, Sendable {
    public var enabled: Bool
    public var startMinute: Int
    public var endMinute: Int
    public var breakThrough: String   // severity that still gets through, e.g. "critical"

    public init(enabled: Bool = false, startMinute: Int = 22 * 60, endMinute: Int = 7 * 60, breakThrough: String = "critical") {
        self.enabled = enabled; self.startMinute = startMinute; self.endMinute = endMinute; self.breakThrough = breakThrough
    }

    public func isQuiet(at date: Date, calendar: Calendar = .current) -> Bool {
        guard enabled, startMinute != endMinute else { return false }
        let c = calendar.dateComponents([.hour, .minute], from: date)
        let m = (c.hour ?? 0) * 60 + (c.minute ?? 0)
        return startMinute < endMinute ? (m >= startMinute && m < endMinute) : (m >= startMinute || m < endMinute)
    }

    public static func rank(_ severity: String) -> Int {
        switch severity.lowercased() {
        case "critical", "high", "error": return 3
        case "warning", "medium": return 2
        case "info", "low": return 1
        default: return 2
        }
    }

    /// Whether a notification of this severity should be shown now.
    public func allows(severity: String, at date: Date, calendar: Calendar = .current) -> Bool {
        !isQuiet(at: date, calendar: calendar) || Self.rank(severity) >= Self.rank(breakThrough)
    }

    public static func label(_ minute: Int) -> String { String(format: "%02d:%02d", minute / 60 % 24, minute % 60) }
}

public struct DoctorFinding: Equatable, Identifiable, Sendable {
    public var id: String { title }
    public var level: HealthLevel
    public var title: String
    public var detail: String
}

public struct VmDoctorReport: Equatable, Sendable {
    public var level: HealthLevel
    public var findings: [DoctorFinding]
    public var lastBackup: Date?
    public var snapshotCount: Int
    public var openAlerts: Int
}

public enum VmDoctor {
    /// Parses the timestamp shapes the daemon uses: ISO 8601 (with or without fractions), `yyyy-MM-dd HH:mm:ss`, epoch seconds or ms.
    public static func parseDate(_ s: String?) -> Date? {
        guard let s = s?.trimmingCharacters(in: .whitespaces), !s.isEmpty else { return nil }
        if let n = Double(s) { return Date(timeIntervalSince1970: n > 1e12 ? n / 1000 : n) }
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let d = f.date(from: s) { return d }
        f.formatOptions = [.withInternetDateTime]
        if let d = f.date(from: s) { return d }
        let p = DateFormatter(); p.locale = Locale(identifier: "en_US_POSIX"); p.timeZone = TimeZone(identifier: "UTC")
        for fmt in ["yyyy-MM-dd HH:mm:ss", "yyyy-MM-dd'T'HH:mm:ss"] { p.dateFormat = fmt; if let d = p.date(from: s) { return d } }
        return nil
    }

    public static func age(_ d: Date, now: Date) -> String {
        let h = Int(now.timeIntervalSince(d) / 3600)
        return h < 1 ? "under an hour ago" : h < 48 ? "\(h) h ago" : "\(h / 24) days ago"
    }

    /// Alerts match a VM when its name appears in the rule name or message.
    public static func assess(vm: VmInfo, backups: [BackupInfo], snapshots: [VmSnapshot], alerts: [FleetAlert],
                              staleBackupDays: Int = 7, now: Date = Date()) -> VmDoctorReport {
        var out: [DoctorFinding] = []
        let mine = backups.filter { ($0.vmFilter ?? "") == vm.name }
        let ok = mine.filter { !["failed", "error"].contains(($0.status ?? "").lowercased()) }
        let last = ok.compactMap { parseDate($0.timestamp) }.max()
        if let last {
            let days = Int(now.timeIntervalSince(last) / 86400)
            if days >= staleBackupDays * 2 { out.append(.init(level: .critical, title: "Backup is stale", detail: "Last good backup \(age(last, now: now)).")) }
            else if days >= staleBackupDays { out.append(.init(level: .warning, title: "Backup is getting old", detail: "Last good backup \(age(last, now: now)).")) }
        } else {
            out.append(.init(level: mine.isEmpty ? .warning : .critical, title: mine.isEmpty ? "No backups" : "No successful backup",
                             detail: mine.isEmpty ? "This VM has never been backed up." : "\(mine.count) backup(s) exist but none succeeded."))
        }
        if snapshots.isEmpty { out.append(.init(level: .ok, title: "No snapshots", detail: "Take one before risky changes.")) }
        else if snapshots.count > 10 { out.append(.init(level: .warning, title: "Many snapshots", detail: "\(snapshots.count) snapshots slow I/O; consider pruning.")) }
        let theirs = alerts.filter { !$0.acknowledged && ($0.message.contains(vm.name) || $0.ruleName.contains(vm.name)) }
        if let worst = theirs.map({ QuietHours.rank($0.severity) }).max() {
            out.append(.init(level: worst >= 3 ? .critical : .warning, title: "\(theirs.count) open alert\(theirs.count == 1 ? "" : "s")",
                             detail: theirs.prefix(3).map(\.message).joined(separator: " · ")))
        }
        let st = vm.state.lowercased()
        if !vm.isRunning && st != "shutoff" && st != "paused" && st != "stopped" {
            out.append(.init(level: .warning, title: "Unusual state", detail: "State is \(vm.state)."))
        }
        return .init(level: out.map(\.level).max() ?? .ok, findings: out.sorted { $0.level > $1.level },
                     lastBackup: last, snapshotCount: snapshots.count, openAlerts: theirs.count)
    }
}

public enum RecentVMs {
    /// Most-recently-used first, unique, capped.
    public static func touching(_ list: [String], _ name: String, limit: Int = 5) -> [String] {
        Array(([name] + list.filter { $0 != name }).prefix(limit))
    }
    /// Drops names that no longer exist.
    public static func existing(_ list: [String], in vms: [VmInfo]) -> [VmInfo] {
        list.compactMap { n in vms.first { $0.name == n } }
    }
}

/// Which actions need Touch ID (or the Mac password) first.
public struct BiometricPolicy: Equatable, Codable, Sendable {
    public var destructiveActions: Bool
    public var approvals: Bool
    public var storedPasswords: Bool
    public init(destructiveActions: Bool = false, approvals: Bool = true, storedPasswords: Bool = false) {
        self.destructiveActions = destructiveActions; self.approvals = approvals; self.storedPasswords = storedPasswords
    }
    public func required(destructive: Bool) -> Bool { destructive && destructiveActions }
}

/// A pinned certificate entry, derived from Keychain account names `pin:HOST:PORT`.
public struct TrustedCert: Equatable, Identifiable, Sendable {
    public var id: String { account }
    public var account: String
    public var host: String
    public var port: Int
    public static func parse(account: String) -> TrustedCert? {
        let p = account.split(separator: ":", omittingEmptySubsequences: false)
        guard p.count == 3, p[0] == "pin", !p[1].isEmpty, let port = Int(p[2]) else { return nil }
        return .init(account: account, host: String(p[1]), port: port)
    }
}
