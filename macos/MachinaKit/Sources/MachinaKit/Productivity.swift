import Foundation

/// Subsequence fuzzy matching for the command palette: every query character must appear in order.
/// Consecutive runs, word starts and early matches score higher.
public enum Fuzzy {
    /// nil when `query` is not a subsequence of `text`. An empty query matches everything with score 0.
    public static func score(_ query: String, in text: String) -> Int? {
        let q = Array(query.lowercased().filter { !$0.isWhitespace }), t = Array(text.lowercased())
        if q.isEmpty { return 0 }
        var qi = 0, score = 0, run = 0, last = -2
        for (i, ch) in t.enumerated() where qi < q.count && ch == q[qi] {
            run = (last == i - 1) ? run + 1 : 0
            score += 1 + run * 3
            if i == 0 || !(t[i - 1].isLetter || t[i - 1].isNumber) { score += 6 }   // word start
            if i < 3 { score += 2 }
            last = i; qi += 1
        }
        guard qi == q.count else { return nil }
        if text.lowercased().contains(String(q)) { score += 20 }   // an exact substring beats a scattered match
        return score - t.count / 8
    }

    /// Items that match, best first (stable for equal scores). Each item is scored on all its `keys`; the best wins.
    public static func rank<T>(_ items: [T], query: String, keys: (T) -> [String]) -> [T] {
        if query.trimmingCharacters(in: .whitespaces).isEmpty { return items }
        return items.enumerated().compactMap { i, item -> (Int, Int, T)? in
            guard let s = keys(item).compactMap({ score(query, in: $0) }).max() else { return nil }
            return (s, i, item)
        }.sorted { $0.0 != $1.0 ? $0.0 > $1.0 : $0.1 < $1.1 }.map { $0.2 }
    }
}

/// Most-recently-used ids, newest first, no duplicates, capped.
public enum Recents {
    public static func pushing(_ id: String, onto list: [String], limit: Int = 8) -> [String] {
        Array(([id] + list.filter { $0 != id }).prefix(limit))
    }
    /// Orders `items` so recent ones come first (in recency order), then the rest in their original order.
    public static func promoting<T>(_ items: [T], recents: [String], id: (T) -> String) -> [T] {
        let rank = Dictionary(recents.enumerated().map { ($1, $0) }, uniquingKeysWith: { a, _ in a })
        let hit = items.filter { rank[id($0)] != nil }.sorted { rank[id($0)]! < rank[id($1)]! }
        return hit + items.filter { rank[id($0)] == nil }
    }
}

/// Pinned (favorite) VM names.
public enum Pins {
    public static func toggling(_ name: String, in pins: [String]) -> [String] {
        pins.contains(name) ? pins.filter { $0 != name } : pins + [name]
    }
    /// Splits the list into pinned VMs (in pin order, skipping pins that no longer exist) and the others.
    public static func split(_ vms: [VmInfo], pins: [String]) -> (pinned: [VmInfo], others: [VmInfo]) {
        let byName = Dictionary(vms.map { ($0.name, $0) }, uniquingKeysWith: { a, _ in a })
        let pinned = pins.compactMap { byName[$0] }
        return (pinned, vms.filter { !pins.contains($0.name) })
    }
}

/// A small query language for the VM list: `is:running`, `is:stopped`, `is:paused`, `vcpus>=4`, `mem>=2048` (MiB),
/// `ip:10.0.`, and any other word as a fuzzy name match. Terms are AND-ed.
public struct VMFilter: Equatable, Sendable {
    public var states: Set<String> = []
    public var minVcpus: Int?
    public var minMemoryMb: Int?
    public var ipPrefix: String?
    public var words: [String] = []

    public init(_ text: String) {
        for raw in text.split(whereSeparator: \.isWhitespace) {
            let t = raw.lowercased()
            if t.hasPrefix("is:"), t.count > 3 { states.insert(String(t.dropFirst(3))) }
            else if t.hasPrefix("vcpus>="), let n = Int(t.dropFirst(7)) { minVcpus = n }
            else if t.hasPrefix("mem>="), let n = Int(t.dropFirst(5)) { minMemoryMb = n }
            else if t.hasPrefix("ip:"), t.count > 3 { ipPrefix = String(t.dropFirst(3)) }
            else { words.append(String(raw)) }
        }
    }

    public var isEmpty: Bool { states.isEmpty && minVcpus == nil && minMemoryMb == nil && ipPrefix == nil && words.isEmpty }

    public func matches(_ vm: VmInfo) -> Bool {
        if !states.isEmpty && !states.contains(vm.state.lowercased()) { return false }
        if let n = minVcpus, vm.vcpus < n { return false }
        if let n = minMemoryMb, vm.memoryMb < n { return false }
        if let p = ipPrefix, !(vm.guestIp ?? "").hasPrefix(p) { return false }
        return words.allSatisfy { Fuzzy.score($0, in: vm.name) != nil }
    }

    public func apply(to vms: [VmInfo]) -> [VmInfo] { isEmpty ? vms : vms.filter(matches) }
}

public struct SavedFilter: Codable, Hashable, Identifiable, Sendable {
    public var name: String
    public var query: String
    public var id: String { name }
    public init(name: String, query: String) { self.name = name; self.query = query }

    /// Same name replaces the old entry; the newest is first; blank names or queries are ignored.
    public static func saving(_ f: SavedFilter, into list: [SavedFilter]) -> [SavedFilter] {
        let name = f.name.trimmingCharacters(in: .whitespaces), q = f.query.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty, !q.isEmpty else { return list }
        return [SavedFilter(name: name, query: q)] + list.filter { $0.name != name }
    }
}

/// Which VMs a bulk action applies to; the rest are skipped with a reason shown in the confirmation.
public enum BulkAction: String, CaseIterable, Sendable, Identifiable {
    case start, shutdown, reboot, pause, resume, forceStop, snapshot
    public var id: String { rawValue }
    public var title: String {
        switch self {
        case .start: "Start"; case .shutdown: "Shut Down"; case .reboot: "Reboot"; case .pause: "Pause"
        case .resume: "Resume"; case .forceStop: "Force Stop"; case .snapshot: "Snapshot"
        }
    }
    public var power: PowerAction? {
        switch self {
        case .start: .start; case .shutdown: .shutdown; case .reboot: .reboot; case .pause: .pause
        case .resume: .resume; case .forceStop: .stop; case .snapshot: nil
        }
    }
    /// Force stop is the only data-risking one; it gets a destructive confirmation.
    public var destructive: Bool { self == .forceStop }

    public func applies(to vm: VmInfo) -> Bool {
        let s = vm.state.lowercased()
        switch self {
        case .start: return s != "running" && s != "paused"
        case .shutdown, .reboot, .pause, .forceStop: return s == "running"
        case .resume: return s == "paused"
        case .snapshot: return true
        }
    }

    public func plan(_ vms: [VmInfo]) -> (apply: [VmInfo], skip: [VmInfo]) {
        (vms.filter(applies), vms.filter { !applies(to: $0) })
    }

    /// `bulk-20260101-120000`: one name for the whole run, valid as a libvirt snapshot name.
    public static func snapshotName(at date: Date) -> String {
        let f = DateFormatter(); f.locale = Locale(identifier: "en_US_POSIX"); f.timeZone = TimeZone(identifier: "UTC")
        f.dateFormat = "yyyyMMdd-HHmmss"
        return "bulk-" + f.string(from: date)
    }
}

/// CSV (RFC 4180) and JSON export of what a resource page shows.
public enum ResourceExport {
    public static func csvField(_ s: String) -> String {
        // A leading = + - @ would be evaluated as a formula by spreadsheets; prefix it.
        var s = s
        if let c = s.first, "=+-@".contains(c), Double(s) == nil { s = "'" + s }
        guard s.contains(where: { $0 == "," || $0 == "\"" || $0 == "\n" || $0 == "\r" }) else { return s }
        return "\"" + s.replacingOccurrences(of: "\"", with: "\"\"") + "\""
    }

    public static func csv(columns: [ResourceColumn], rows: [ResourceRow]) -> String {
        let head = columns.map { csvField($0.title) }.joined(separator: ",")
        let body = rows.map { r in columns.map { c in let v = c.render(r); return csvField(v == "–" ? "" : v) }.joined(separator: ",") }
        return ([head] + body).joined(separator: "\r\n") + "\r\n"
    }

    /// The full raw rows as pretty JSON (not just the visible columns).
    public static func json(rows: [ResourceRow]) -> String {
        let enc = JSONEncoder(); enc.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = (try? enc.encode(JSONValue.array(rows.map(\.values)))) ?? Data("[]".utf8)
        return String(decoding: data, as: UTF8.self)
    }

    public static func fileName(_ title: String, ext: String) -> String {
        let slug = title.lowercased().map { $0.isLetter || $0.isNumber ? String($0) : "-" }.joined()
            .split(separator: "-").joined(separator: "-")
        return (slug.isEmpty ? "export" : slug) + "." + ext
    }
}

/// `curl` command for any request the app makes. Credentials are never embedded: the session cookie comes
/// from a login line that reads the password from the environment.
public enum Curl {
    static func quote(_ s: String) -> String { "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'" }

    public static func url(_ e: Endpoint, server: URL) -> String {
        var c = URLComponents(url: server, resolvingAgainstBaseURL: false) ?? URLComponents()
        c.percentEncodedPath = e.resolvedPath
        c.queryItems = e.query.isEmpty ? nil : e.query
        return c.string ?? server.absoluteString + e.resolvedPath
    }

    /// One `curl` line using a cookie jar (`-b`), `-k` because hosts commonly use self-signed certificates.
    public static func command(_ e: Endpoint, server: URL, cookieJar: String = "$MACHINA_JAR") -> String {
        var parts = ["curl", "-sk", "-b", cookieJar, "-X", e.method]
        if let body = e.body {
            let enc = JSONEncoder(); enc.outputFormatting = [.sortedKeys]
            parts += ["-H", quote("Content-Type: application/json"), "-d", quote(String(decoding: (try? enc.encode(SecretRedaction.redact(body))) ?? Data(), as: UTF8.self))]
        }
        parts.append(quote(url(e, server: server)))
        return parts.joined(separator: " ")
    }

    /// Login line + request, ready to paste into a shell (`MACHINA_USER` / `MACHINA_PASS` come from the environment).
    public static func script(_ e: Endpoint, server: URL) -> String {
        let login = "MACHINA_JAR=$(mktemp); curl -sk -c \"$MACHINA_JAR\" -X POST -H 'Content-Type: application/json' "
            + "-d \"{\\\"username\\\":\\\"$MACHINA_USER\\\",\\\"password\\\":\\\"$MACHINA_PASS\\\"}\" "
            + quote(url(Endpoint("POST", "/api/v1/auth/login"), server: server)) + " >/dev/null"
        return login + "\n" + command(e, server: server)
    }
}

/// Keyboard shortcut catalog shown by the cheat sheet.
public struct ShortcutInfo: Hashable, Sendable, Identifiable {
    public var keys: String
    public var title: String
    public var group: String
    public var id: String { keys + title }

    public static let all: [ShortcutInfo] = [
        .init(keys: "⌘K", title: "Command palette", group: "Navigate"),
        .init(keys: "⌘1", title: "Fleet", group: "Navigate"),
        .init(keys: "⌘2", title: "Tasks", group: "Navigate"),
        .init(keys: "⌘3", title: "Alerts", group: "Navigate"),
        .init(keys: "⌘N", title: "New VM", group: "VMs"),
        .init(keys: "⇧⌘B", title: "Bulk actions on VMs", group: "VMs"),
        .init(keys: "⇧⌘P", title: "Pin or unpin the selected VM", group: "VMs"),
        .init(keys: "⇧⌘S", title: "Save the current VM filter", group: "VMs"),
        .init(keys: "⌘R", title: "Refresh", group: "General"),
        .init(keys: "⇧⌘E", title: "Export the current page as CSV", group: "General"),
        .init(keys: "⇧⌘C", title: "Copy the current page request as curl", group: "General"),
        .init(keys: "⌘/", title: "Keyboard shortcuts", group: "General"),
        .init(keys: "↩", title: "Run the top palette result", group: "Palette"),
        .init(keys: "⎋", title: "Close the palette or a dialog", group: "Palette"),
    ]
}
