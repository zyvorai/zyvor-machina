import Foundation

/// Pages of the "Visual insight" sidebar section.
public enum InsightPage: String, CaseIterable, Hashable, Sendable {
    case topology, grid, heatmap, timeline
    public var title: String {
        switch self { case .topology: "Topology map"; case .grid: "VM grid"; case .heatmap: "Capacity heatmap"; case .timeline: "What changed" }
    }
    public var icon: String {
        switch self { case .topology: "point.3.connected.trianglepath.dotted"; case .grid: "square.grid.3x3.fill"; case .heatmap: "square.grid.3x3.topleft.filled"; case .timeline: "clock.arrow.circlepath" }
    }
}

/// Coarse health colour shared by every insight view.
public enum Tone: String, Hashable, Sendable, CaseIterable {
    case ok, warn, bad, idle

    public static func vm(_ state: String) -> Tone {
        switch state.lowercased() {
        case "running": .ok
        case "paused", "pmsuspended", "suspended", "blocked", "in_shutdown", "migrating": .warn
        case "crashed", "error", "failed": .bad
        default: .idle
        }
    }

    public static func host(_ h: PlatformHost) -> Tone {
        if !h.isOnline || h.fenced == true { return .bad }
        if h.maintenanceMode == true { return .warn }
        return .ok
    }

    /// Utilisation percent: >=90 bad, >=75 warn.
    public static func load(_ pct: Double?) -> Tone {
        guard let p = pct else { return .idle }
        return p >= 90 ? .bad : p >= 75 ? .warn : .ok
    }
    var rank: Int { switch self { case .bad: 0; case .warn: 1; case .ok: 2; case .idle: 3 } }
}

/// A VM as the controller sees it (`GET /api/v1/vms` through the controller proxy): carries the owning host.
public struct PlatformVM: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var name: String
    public var hostId: String?
    public var observedState: String
    public var vcpus: Int
    public var memoryMib: Int

    public init(id: String, name: String, hostId: String?, observedState: String, vcpus: Int, memoryMib: Int) {
        self.id = id; self.name = name; self.hostId = hostId; self.observedState = observedState; self.vcpus = vcpus; self.memoryMib = memoryMib
    }
    public init(_ vm: VmInfo, hostId: String? = nil) {
        self.init(id: vm.name, name: vm.name, hostId: hostId, observedState: vm.state, vcpus: vm.vcpus, memoryMib: vm.memoryMb)
    }
    public var tone: Tone { Tone.vm(observedState) }

    enum CodingKeys: String, CodingKey { case id, name, vcpus; case hostId = "host_id", observedState = "observed_state", memoryMib = "memory_mib" }
    public init(from d: Decoder) throws {
        let c = try d.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        hostId = try c.decodeIfPresent(String.self, forKey: .hostId)
        observedState = try c.decodeIfPresent(String.self, forKey: .observedState) ?? "unknown"
        vcpus = try c.decodeIfPresent(Int.self, forKey: .vcpus) ?? 0
        memoryMib = try c.decodeIfPresent(Int.self, forKey: .memoryMib) ?? 0
    }
}

public extension MachinaClient {
    func fleetVMs() async throws -> [PlatformVM] {
        try JSONDecoder().decode([PlatformVM].self, from: try await send("GET", "/api/v1/platform/controller/api/v1/vms"))
    }
}

// MARK: Topology

/// One host and the VMs placed on it. `host == nil` is the group of VMs whose host is unknown.
public struct HostGroup: Identifiable, Hashable, Sendable {
    public var host: PlatformHost?
    public var name: String
    public var vms: [PlatformVM]
    public var id: String { host?.id ?? "unassigned" }
    public var running: Int { vms.filter { $0.tone == .ok }.count }
    public var vcpus: Int { vms.reduce(0) { $0 + $1.vcpus } }
    public var memoryMib: Int { vms.reduce(0) { $0 + $1.memoryMib } }
    public var tone: Tone { host.map(Tone.host) ?? .idle }
}

public enum Topology {
    /// Hosts (online first, then by name) with their VMs (problems first, then by name).
    /// VMs whose host is not in `hosts` end up in a trailing "Unassigned" group.
    public static func group(hosts: [PlatformHost], vms: [PlatformVM], unassignedName: String = "Unassigned") -> [HostGroup] {
        let byHost = Dictionary(grouping: vms) { $0.hostId ?? "" }
        func order(_ v: [PlatformVM]) -> [PlatformVM] {
            v.sorted { ($0.tone.rank, $0.name.lowercased()) < ($1.tone.rank, $1.name.lowercased()) }
        }
        var out = hosts.sorted { a, b in
            (a.isOnline ? 0 : 1, a.hostname.lowercased()) < (b.isOnline ? 0 : 1, b.hostname.lowercased())
        }.map { HostGroup(host: $0, name: $0.hostname, vms: order(byHost[$0.id] ?? [])) }
        let known = Set(hosts.map(\.id))
        let orphans = vms.filter { !known.contains($0.hostId ?? "") }
        if !orphans.isEmpty { out.append(HostGroup(host: nil, name: unassignedName, vms: order(orphans))) }
        return out
    }
}

// MARK: Capacity heatmap

public enum Heat {
    /// 0 (cold) ... 4 (hot); nil when there is no reading.
    public static func level(_ pct: Double?) -> Int? {
        guard let p = pct, p.isFinite else { return nil }
        switch p { case ..<25: return 0; case ..<50: return 1; case ..<70: return 2; case ..<85: return 3; default: return 4 }
    }

    /// Memory the running VMs were given, as a percent of the host's RAM (can exceed 100 when overcommitted).
    public static func memoryCommitPercent(_ g: HostGroup) -> Double? {
        guard let t = g.host?.memoryTotalMib, t > 0 else { return nil }
        let given = g.vms.filter { $0.tone == .ok }.reduce(0) { $0 + $1.memoryMib }
        return Double(given) / t * 100
    }

    /// How many more VMs of `sizeMib` fit in the memory that is neither used nor committed. Offline or fenced hosts fit none.
    public static func headroom(_ g: HostGroup, sizeMib: Int = 2048) -> Int {
        guard let h = g.host, h.isOnline, h.fenced != true, h.schedulable != false, let t = h.memoryTotalMib, sizeMib > 0 else { return 0 }
        let used = max(h.memoryUsedMib ?? 0, t * (memoryCommitPercent(g) ?? 0) / 100)
        return max(0, Int((t - used) / Double(sizeMib)))
    }

    /// Averages `values` into at most `columns` buckets (oldest first), so a long history fits a fixed-width strip.
    public static func bucketed(_ values: [Double], columns: Int) -> [Double] {
        guard columns > 0, !values.isEmpty else { return [] }
        if values.count <= columns { return values }
        return (0..<columns).map { i in
            let lo = i * values.count / columns, hi = max((i + 1) * values.count / columns, lo + 1)
            let s = values[lo..<hi]
            return s.reduce(0, +) / Double(s.count)
        }
    }
}

// MARK: VM grid

public enum Spark {
    /// The most recent `count` CPU percentages of a VM (oldest first); empty when the history has no usable pair.
    public static func cpu(_ points: [MetricsPoint], vm: String, count: Int = 30) -> [Double] {
        Array(MetricsMath.vmCPU(points, vm: vm).map(\.value).suffix(count))
    }
    public static func memoryPercent(_ points: [MetricsPoint], vm: String, count: Int = 30) -> [Double] {
        Array(points.sorted { $0.timestampMs < $1.timestampMs }.compactMap { p in
            p.vmMetrics?.first { $0.name == vm }?.memoryPct
        }.suffix(count))
    }

    public enum Order: String, CaseIterable, Sendable { case name = "Name", cpu = "CPU", state = "State" }

    /// Orders VMs for the grid. `cpu` is the latest CPU percent per VM name; VMs without a reading sort last under CPU.
    public static func order(_ vms: [VmInfo], by o: Order, cpu: [String: Double]) -> [VmInfo] {
        switch o {
        case .name: return vms.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
        case .cpu:
            return vms.sorted { a, b in
                let x = cpu[a.name] ?? -1, y = cpu[b.name] ?? -1
                return x != y ? x > y : a.name < b.name
            }
        case .state:
            return vms.sorted { a, b in
                let x = Tone.vm(a.state).rank, y = Tone.vm(b.state).rank
                return x != y ? x < y : a.name < b.name
            }
        }
    }
}

// MARK: Activity timeline (from the watch stream)

public struct ActivityEntry: Identifiable, Hashable, Sendable {
    public var id = UUID()
    public var date: Date
    public var vm: String
    public var event: String
    public var oldState: String?
    public var newState: String?

    public var tone: Tone {
        if let n = newState { return Tone.vm(n) }
        switch event { case "removed", "deleted", "undefined": return .bad; case "added", "created", "defined": return .ok; default: return .idle }
    }
    public var summary: String {
        switch (oldState, newState) {
        case let (o?, n?): return "\(o) \u{2192} \(n)"
        case let (nil, n?): return n
        default: return event.replacingOccurrences(of: "_", with: " ")
        }
    }
}

public struct ActivityBucket: Identifiable, Hashable, Sendable {
    public var start: Date
    public var tone: Tone
    public var count: Int
    public var id: String { "\(start.timeIntervalSince1970)-\(tone.rawValue)" }
}

public enum ActivityLog {
    /// Prepends `changes` (newest first) and keeps at most `cap` entries.
    public static func append(_ log: [ActivityEntry], _ changes: [VmChange], at date: Date, cap: Int = 500) -> [ActivityEntry] {
        let new = changes.reversed().map { ActivityEntry(date: date, vm: $0.name, event: $0.event, oldState: $0.oldState, newState: $0.newState) }
        return Array((new + log).prefix(cap))
    }

    /// Counts per time slot and tone for the last `slots` slots ending at `now` (slots with nothing are omitted).
    public static func buckets(_ log: [ActivityEntry], slot: TimeInterval, slots: Int, now: Date) -> [ActivityBucket] {
        guard slot > 0, slots > 0 else { return [] }
        let end = (now.timeIntervalSince1970 / slot).rounded(.down) + 1
        let first = end - Double(slots)
        var counts: [String: ActivityBucket] = [:]
        for e in log {
            let idx = (e.date.timeIntervalSince1970 / slot).rounded(.down)
            guard idx >= first, idx < end else { continue }
            let start = Date(timeIntervalSince1970: idx * slot)
            let key = "\(idx)-\(e.tone.rawValue)"
            counts[key, default: ActivityBucket(start: start, tone: e.tone, count: 0)].count += 1
        }
        return counts.values.sorted { a, b in a.start != b.start ? a.start < b.start : a.tone.rank < b.tone.rank }
    }

    /// Entries whose VM name contains `vm`, or all of them for an empty filter.
    public static func filter(_ log: [ActivityEntry], vm: String) -> [ActivityEntry] {
        vm.isEmpty ? log : log.filter { $0.vm.localizedCaseInsensitiveContains(vm) }
    }
}
