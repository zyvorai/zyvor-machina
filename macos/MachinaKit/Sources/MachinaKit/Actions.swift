import Foundation

// MARK: Models

public struct VmDisk: Codable, Hashable, Identifiable, Sendable {
    public var device: String?
    public var source: String?
    public var driver: String?
    public var target: String
    public var bus: String?
    public var cache: String?
    public var readonly: Bool?
    public var id: String { target }
    public var isCdrom: Bool { device == "cdrom" }
}

public struct VmNic: Codable, Hashable, Identifiable, Sendable {
    public var macAddress: String
    public var source: String?
    public var model: String?
    public var id: String { macAddress }
    enum CodingKeys: String, CodingKey { case macAddress = "mac_address", source, model }
}

/// `GET /api/v1/vms/{name}` (mirrors `VmDetails` in `core/src/state.rs`; everything but the name is optional).
public struct VmDetails: Codable, Hashable, Sendable {
    public var name: String
    public var uuid: String?
    public var state: String?
    public var vcpus: Int?
    public var memoryMb: Int?
    public var osType: String?
    public var arch: String?
    public var autostart: Bool?
    public var persistent: Bool?
    public var interfaces: [VmNic]?
    public var disks: [VmDisk]?
    public var guestIp: String?
    enum CodingKeys: String, CodingKey {
        case name, uuid, state, vcpus, arch, autostart, persistent, interfaces, disks
        case memoryMb = "memory_mb", osType = "os_type", guestIp = "guest_ip"
    }
}

public struct VmSnapshot: Codable, Hashable, Identifiable, Sendable {
    public var name: String
    public var vmName: String?
    public var creationTime: Double?
    public var state: String?
    public var description: String?
    public var parent: String?
    public var isCurrent: Bool?
    public var id: String { name }
    enum CodingKeys: String, CodingKey {
        case name, state, description, parent
        case vmName = "vm_name", creationTime = "creation_time", isCurrent = "is_current"
    }
}

public struct BackupInfo: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var timestamp: String?
    public var vmFilter: String?
    public var size: String?
    public var status: String?
    public var withDisks: Bool?
    enum CodingKeys: String, CodingKey { case id, timestamp, size, status, vmFilter = "vm_filter", withDisks = "with_disks" }
}

public struct StorageVolume: Codable, Hashable, Identifiable, Sendable {
    public var name: String
    public var pool: String?
    public var capacityGb: Double?
    public var allocationGb: Double?
    public var path: String?
    public var volType: String?
    public var id: String { path ?? name }
    enum CodingKeys: String, CodingKey { case name, pool, path, capacityGb = "capacity_gb", allocationGb = "allocation_gb", volType = "vol_type" }
}

public enum CloneMode: String, CaseIterable, Sendable { case linked, full, xml }

public struct CreateVMRequest: Sendable {
    public var name: String
    public var vcpus: Int
    public var memoryMb: Int
    public var diskGb: Int
    public var iso: String?
    public var network: String?
    public var osVariant: String?
    public var savedTemplate: String?
    public init(name: String, vcpus: Int, memoryMb: Int, diskGb: Int, iso: String? = nil, network: String? = nil,
                osVariant: String? = nil, savedTemplate: String? = nil) {
        self.name = name; self.vcpus = vcpus; self.memoryMb = memoryMb; self.diskGb = diskGb
        self.iso = iso; self.network = network; self.osVariant = osVariant; self.savedTemplate = savedTemplate
    }
    var json: JSONValue {
        var o: [String: JSONValue] = ["name": .string(name), "vcpus": .number(Double(vcpus)),
                                      "memory_mb": .number(Double(memoryMb)), "disk_gb": .number(Double(diskGb))]
        if let iso, !iso.isEmpty { o["iso"] = .string(iso) }
        if let network, !network.isEmpty { o["network"] = .string(network) }
        if let osVariant, !osVariant.isEmpty { o["os_variant"] = .string(osVariant) }
        if let savedTemplate, !savedTemplate.isEmpty { o["saved_template"] = .string(savedTemplate) }
        return .object(o)
    }
}

// MARK: Calls (all writes need an operator/admin session; the server enforces that)

public extension MachinaClient {
    private func vm(_ method: String, _ vm: VmInfo, _ suffix: String = "", body: JSONValue? = nil) async throws -> JSONValue {
        try await call(Endpoint(method, "/api/v1/vms/\(Self.seg(vm.name))\(suffix)", query: vm.scopeQuery, body: body))
    }
    private func decode<T: Decodable>(_ type: T.Type, _ v: JSONValue) throws -> T {
        try JSONDecoder().decode(T.self, from: try JSONEncoder().encode(v))
    }

    func details(_ v: VmInfo) async throws -> VmDetails { try decode(VmDetails.self, try await vm("GET", v)) }

    // Snapshots
    func snapshots(_ v: VmInfo) async throws -> [VmSnapshot] { try decode([VmSnapshot].self, try await vm("GET", v, "/snapshots")) }
    func createSnapshot(_ v: VmInfo, name: String, description: String = "") async throws {
        var o: [String: JSONValue] = ["name": .string(name)]
        if !description.isEmpty { o["description"] = .string(description) }
        _ = try await vm("POST", v, "/snapshots", body: .object(o))
    }
    func revertSnapshot(_ v: VmInfo, _ snap: String) async throws { _ = try await vm("POST", v, "/snapshots/\(Self.seg(snap))/revert") }
    func deleteSnapshot(_ v: VmInfo, _ snap: String) async throws { _ = try await vm("DELETE", v, "/snapshots/\(Self.seg(snap))") }

    // Lifecycle and settings
    func clone(_ v: VmInfo, as newName: String, mode: CloneMode = .linked) async throws {
        _ = try await vm("POST", v, "/clone", body: .object(["new_name": .string(newName), "clone_mode": .string(mode.rawValue)]))
    }
    func rename(_ v: VmInfo, to newName: String) async throws { _ = try await vm("POST", v, "/rename", body: .object(["new_name": .string(newName)])) }
    func setAutostart(_ v: VmInfo, _ on: Bool) async throws { _ = try await vm("POST", v, "/autostart/\(on)") }
    func setVcpus(_ v: VmInfo, _ n: Int) async throws { _ = try await vm("POST", v, "/vcpus/\(n)") }
    func setMemory(_ v: VmInfo, mb: Int) async throws { _ = try await vm("POST", v, "/memory/\(mb)") }
    func deleteVM(_ v: VmInfo) async throws { _ = try await vm("DELETE", v) }
    func createVM(_ r: CreateVMRequest) async throws { _ = try await call(Endpoint("POST", "/api/v1/vms", body: r.json)) }

    // Disks, media and NICs
    /// libvirt needs an existing absolute `source` path (see `volumes(pool:)`); a bare `sizeGb` only works for FluxVM.
    func attachDisk(_ v: VmInfo, target: String, source: String = "", sizeGb: Int = 0, bus: String? = nil) async throws {
        var o: [String: JSONValue] = ["source": .string(source), "target": .string(target), "size_gb": .number(Double(sizeGb))]
        if let bus { o["bus"] = .string(bus) }
        _ = try await vm("POST", v, "/disk/attach", body: .object(o))
    }
    func detachDisk(_ v: VmInfo, target: String) async throws { _ = try await vm("POST", v, "/disk/detach/\(Self.seg(target))") }
    func resizeDisk(_ v: VmInfo, target: String, sizeGb: Int) async throws {
        _ = try await vm("POST", v, "/disk/resize/\(Self.seg(target))", body: .object(["size_gb": .number(Double(sizeGb))]))
    }
    func insertCdrom(_ v: VmInfo, iso: String, target: String = "") async throws {
        _ = try await vm("POST", v, "/cdrom/insert", body: .object(["iso_path": .string(iso), "target": .string(target)]))
    }
    func ejectCdrom(_ v: VmInfo, target: String) async throws { _ = try await vm("POST", v, "/cdrom/eject/\(Self.seg(target))") }
    func attachNic(_ v: VmInfo, network: String, model: String = "virtio") async throws {
        _ = try await vm("POST", v, "/nic/attach", body: .object(["network": .string(network), "model": .string(model)]))
    }
    func detachNic(_ v: VmInfo, mac: String) async throws { _ = try await vm("POST", v, "/nic/detach/\(Self.seg(mac))") }

    // Storage volumes
    func volumes(pool: String) async throws -> [StorageVolume] {
        try decode([StorageVolume].self, try await call(.get("/api/v1/storage/pools/\(Self.seg(pool))/volumes")))
    }
    func createVolume(pool: String, name: String, capacityGb: Int, format: String = "qcow2") async throws {
        _ = try await call(Endpoint("POST", "/api/v1/storage/pools/\(Self.seg(pool))/volumes",
                                    body: .object(["name": .string(name), "capacity_gb": .number(Double(capacityGb)), "format": .string(format)])))
    }
    func deleteVolume(pool: String, name: String) async throws {
        _ = try await call(Endpoint("DELETE", "/api/v1/storage/pools/\(Self.seg(pool))/volumes/\(Self.seg(name))"))
    }

    // Backups
    func backups() async throws -> [BackupInfo] { try decode([BackupInfo].self, try await call(.get("/api/v1/backups"))) }
    func backUp(_ v: VmInfo, withDisks: Bool) async throws {
        _ = try await call(Endpoint("POST", "/api/v1/backups", body: .object(["vm_name": .string(v.name), "with_disks": .bool(withDisks)])))
    }
    func restoreBackup(_ id: String) async throws {
        _ = try await call(Endpoint("POST", "/api/v1/backups/restore", body: .object(["backup_id": .string(id)])))
    }
    func deleteBackup(_ id: String) async throws { _ = try await call(Endpoint("DELETE", "/api/v1/backups/\(Self.seg(id))")) }
    func verifyBackup(_ id: String) async throws -> JSONValue { try await call(Endpoint("POST", "/api/v1/backups/\(Self.seg(id))/verify")) }
}
