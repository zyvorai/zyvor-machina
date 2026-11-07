import Foundation

/// Mirrors `core/src/state.rs` `VmInfo` (daemon `GET /api/v1/vms`).
public struct VmInfo: Codable, Hashable, Identifiable, Sendable {
    public var name: String
    public var state: String
    public var vcpus: Int
    public var memoryMb: Int
    public var libvirtConnection: String?
    public var guestIp: String?
    public var guestIps: [String]?
    public var backend: String?
    public var fluxvmBackend: String?

    public var id: String { name }
    public var isRunning: Bool { state.lowercased() == "running" }

    enum CodingKeys: String, CodingKey {
        case name, state, vcpus, backend
        case memoryMb = "memory_mb"
        case libvirtConnection = "libvirt_connection"
        case guestIp = "guest_ip"
        case guestIps = "guest_ips"
        case fluxvmBackend = "fluxvm_backend"
    }

    /// Query string that scopes a per-VM call (`?connection=session` / `?backend=fluxvm`).
    public var scopeQuery: [URLQueryItem] {
        guard let c = libvirtConnection, !c.isEmpty, c != "system" else { return [] }
        return c == "fluxvm"
            ? [URLQueryItem(name: "backend", value: "fluxvm")]
            : [URLQueryItem(name: "connection", value: c)]
    }
}

public enum PowerAction: String, CaseIterable, Sendable {
    case start, stop, shutdown, reboot, pause, resume
    public var isDestructive: Bool { self == .stop }
}

public struct SessionInfo: Codable, Sendable {
    public var authenticated: Bool?
    public var username: String?
    public var role: String?
}

public struct WsToken: Codable, Sendable { public var token: String }

/// One entry of a `/ws/v1/watch` `changes` message.
public struct VmChange: Codable, Hashable, Sendable {
    public var event: String
    public var name: String
    public var oldState: String?
    public var newState: String?
    enum CodingKeys: String, CodingKey {
        case event, name
        case oldState = "old_state", newState = "new_state"
    }
}

public enum WatchEvent: Sendable {
    case heartbeat
    case changes([VmChange])
    case disconnected
}

public struct ServerProfile: Codable, Hashable, Identifiable, Sendable {
    public var id: UUID
    public var name: String
    public var url: URL
    public init(id: UUID = UUID(), name: String, url: URL) {
        self.id = id; self.name = name; self.url = url
    }
}
