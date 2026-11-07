import Foundation

// MARK: Models (controller + fleet endpoints)

/// Controller host row (`GET /api/v1/hosts` through the platform proxy).
public struct PlatformHost: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var hostname: String
    public var address: String?
    public var state: String
    public var maintenanceMode: Bool?
    public var schedulable: Bool?
    public var fenced: Bool?
    public var vmCount: Int?
    public var cpuPercent: Double?
    public var memoryUsedMib: Double?
    public var memoryTotalMib: Double?
    public var lastHeartbeatAt: String?

    public var isOnline: Bool { state.lowercased() == "online" }
    public var memoryPercent: Double? {
        guard let u = memoryUsedMib, let t = memoryTotalMib, t > 0 else { return nil }
        return u / t * 100
    }

    enum CodingKeys: String, CodingKey {
        case id, hostname, address, state, schedulable, fenced
        case maintenanceMode = "maintenance_mode", vmCount = "vm_count", cpuPercent = "cpu_percent"
        case memoryUsedMib = "memory_used_mib", memoryTotalMib = "memory_total_mib"
        case lastHeartbeatAt = "last_heartbeat_at"
    }
}

/// Controller task (`GET /api/v1/tasks`).
public struct PlatformTask: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var operation: String
    public var status: String
    public var progress: Double?
    public var message: String?
    public var createdAt: String

    public var isActive: Bool { ["pending", "queued", "running", "in_progress"].contains(status.lowercased()) }
    public var isFailed: Bool { ["failed", "error"].contains(status.lowercased()) }

    enum CodingKeys: String, CodingKey {
        case id, operation, status, progress, message
        case createdAt = "created_at"
    }
}

public struct FleetAlert: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var ruleName: String
    public var message: String
    public var severity: String
    public var timestamp: String
    public var acknowledged: Bool
    public var peer: String?   // filled in when flattening the per-peer envelope

    enum CodingKeys: String, CodingKey {
        case id, message, severity, timestamp, acknowledged, peer
        case ruleName = "rule_name"
    }
}

public struct FleetAlerts: Sendable {
    public var totalUnacknowledged: Int
    public var alerts: [FleetAlert]
}

private struct AlertsEnvelope: Decodable {
    struct Peer: Decodable { var peer: String?; var alerts: [FleetAlert] }
    var totalUnacknowledged: Int?
    var peers: [Peer]
    enum CodingKeys: String, CodingKey { case totalUnacknowledged = "total_unacknowledged", peers }
}

public struct Capacity: Codable, Hashable, Sendable { public var score: Double?; public var label: String? }

/// `local` block of `GET /api/v1/fleet/metrics` (this daemon's host).
public struct LocalMetrics: Codable, Hashable, Sendable {
    public var hostCpuPercent: Double?
    public var hostMemoryPercent: Double?
    public var hostDiskPercent: Double?
    public var load1: Double?
    public var vmCount: Int?
    public var vmsRunning: Int?
    public var capacity: Capacity?
    enum CodingKeys: String, CodingKey {
        case hostCpuPercent = "host_cpu_percent", hostMemoryPercent = "host_memory_percent"
        case hostDiskPercent = "host_disk_percent", load1 = "load_1"
        case vmCount = "vm_count", vmsRunning = "vms_running", capacity
    }
}

private struct MetricsEnvelope: Decodable { var local: LocalMetrics }

// MARK: Client calls

public extension MachinaClient {
    /// Controller routes go through the daemon's reverse proxy (never :5093 directly).
    private static let proxy = "/api/v1/platform/controller"

    func hosts() async throws -> [PlatformHost] {
        try JSONDecoder().decode([PlatformHost].self, from: try await send("GET", Self.proxy + "/api/v1/hosts"))
    }

    func tasks(limit: Int = 100) async throws -> [PlatformTask] {
        let data = try await send("GET", Self.proxy + "/api/v1/tasks", query: [URLQueryItem(name: "limit", value: String(limit))])
        return try JSONDecoder().decode([PlatformTask].self, from: data)
    }

    /// Needs a write role; the server answers 403 otherwise.
    func cancelTask(_ id: String) async throws { _ = try await send("POST", Self.proxy + "/api/v1/tasks/\(Self.seg(id))/cancel") }
    func retryTask(_ id: String) async throws { _ = try await send("POST", Self.proxy + "/api/v1/tasks/\(Self.seg(id))/retry") }

    func localMetrics() async throws -> LocalMetrics {
        try JSONDecoder().decode(MetricsEnvelope.self, from: try await send("GET", "/api/v1/fleet/metrics")).local
    }

    func alerts() async throws -> FleetAlerts {
        let env = try JSONDecoder().decode(AlertsEnvelope.self, from: try await send("GET", "/api/v1/fleet/alerts"))
        let flat = env.peers.flatMap { p in p.alerts.map { a -> FleetAlert in var a = a; a.peer = p.peer; return a } }
        return FleetAlerts(totalUnacknowledged: env.totalUnacknowledged ?? flat.filter { !$0.acknowledged }.count, alerts: flat)
    }

    public static func seg(_ s: String) -> String {
        var set = CharacterSet.urlPathAllowed; set.remove("/")
        return s.addingPercentEncoding(withAllowedCharacters: set) ?? s
    }
}
