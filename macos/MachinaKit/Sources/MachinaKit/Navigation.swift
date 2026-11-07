import Foundation

/// `machina://vm/<name>`, `machina://page/<id>`, `machina://fleet|tasks|alerts`: links from Spotlight, Shortcuts or other apps.
public enum DeepLink: Equatable, Sendable {
    case vm(String), page(String), fleet, tasks, alerts

    public init?(url: URL) {
        guard url.scheme?.lowercased() == "machina", let host = url.host?.lowercased() else { return nil }
        let rest = url.pathComponents.filter { $0 != "/" }
        switch (host, rest.first) {
        case ("vm", let n?): self = .vm(n)
        case ("page", let id?): self = .page(id)
        case ("fleet", _): self = .fleet
        case ("tasks", _): self = .tasks
        case ("alerts", _): self = .alerts
        default: return nil
        }
    }

    public var url: URL {
        var c = URLComponents(); c.scheme = "machina"
        switch self {
        case .vm(let n): c.host = "vm"; c.path = "/" + n
        case .page(let id): c.host = "page"; c.path = "/" + id
        case .fleet: c.host = "fleet"
        case .tasks: c.host = "tasks"
        case .alerts: c.host = "alerts"
        }
        return c.url!
    }
}

/// The saved-server list: one entry per server and user, newest use first.
public enum ServerList {
    /// Same scheme, host and port (case-insensitive) and the same user count as the same server.
    public static func sameServer(_ a: ServerProfile, url: URL, username: String?) -> Bool {
        func key(_ u: URL) -> String { "\(u.scheme?.lowercased() ?? "")://\(u.host?.lowercased() ?? ""):\(u.port ?? (u.scheme == "http" ? 80 : 443))" }
        return key(a.url) == key(url) && (a.username ?? "") == (username ?? "")
    }

    public static func remembering(_ list: [ServerProfile], url: URL, username: String?, name: String? = nil) -> [ServerProfile] {
        var out = list
        if let i = out.firstIndex(where: { sameServer($0, url: url, username: username) }) {
            var p = out.remove(at: i); p.url = url
            if let name, !name.isEmpty { p.name = name }
            out.insert(p, at: 0)
        } else {
            out.insert(ServerProfile(name: name ?? url.host ?? "Server", url: url, username: username), at: 0)
        }
        return Array(out.prefix(20))
    }

    public static func removing(_ list: [ServerProfile], id: UUID) -> [ServerProfile] { list.filter { $0.id != id } }
}
