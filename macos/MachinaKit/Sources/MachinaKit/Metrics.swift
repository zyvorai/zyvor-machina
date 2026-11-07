import Foundation

public struct VmMetricsSample: Codable, Hashable, Sendable {
    public var name: String
    public var state: String?
    public var cpuTimeNs: Double?
    public var vcpus: Int?
    public var memoryTotalMb: Double?
    public var memoryUsedMb: Double?
    public var memoryPct: Double?
    public var diskRdBytes: Double?
    public var diskWrBytes: Double?
    public var netRxBytes: Double?
    public var netTxBytes: Double?
    enum CodingKeys: String, CodingKey {
        case name, state, vcpus
        case cpuTimeNs = "cpu_time_ns", memoryTotalMb = "memory_total_mb", memoryUsedMb = "memory_used_mb", memoryPct = "memory_pct"
        case diskRdBytes = "disk_rd_bytes", diskWrBytes = "disk_wr_bytes", netRxBytes = "net_rx_bytes", netTxBytes = "net_tx_bytes"
    }
}

/// One sample of `GET /api/v1/metrics/history`: host totals plus a counter snapshot per VM.
public struct MetricsPoint: Codable, Hashable, Identifiable, Sendable {
    public var timestampMs: Double
    public var hostCpuPercent: Double?
    public var hostMemoryPercent: Double?
    public var hostDiskPercent: Double?
    public var load1: Double?
    public var vmsRunning: Int?
    public var vmCount: Int?
    public var vmMetrics: [VmMetricsSample]?
    public var id: Double { timestampMs }
    public var date: Date { Date(timeIntervalSince1970: timestampMs / 1000) }
    enum CodingKeys: String, CodingKey {
        case timestampMs = "timestamp_ms", hostCpuPercent = "host_cpu_percent", hostMemoryPercent = "host_memory_percent"
        case hostDiskPercent = "host_disk_percent", load1 = "load_1", vmsRunning = "vms_running", vmCount = "vm_count", vmMetrics = "vm_metrics"
    }
}

public struct SeriesPoint: Identifiable, Hashable, Sendable {
    public var date: Date
    public var value: Double
    public var series: String
    public var id: String { "\(series)@\(date.timeIntervalSince1970)" }
}

/// Turns raw samples into chart series. The server's CPU time and byte counters are cumulative, so rates come
/// from consecutive samples; a counter that went backwards (VM restart) or a zero time gap is skipped.
public enum MetricsMath {
    public enum Counter: String, Sendable, CaseIterable {
        case diskRead = "Disk read", diskWrite = "Disk write", netRx = "Network in", netTx = "Network out"
        fileprivate func value(_ s: VmMetricsSample) -> Double? {
            switch self { case .diskRead: s.diskRdBytes; case .diskWrite: s.diskWrBytes; case .netRx: s.netRxBytes; case .netTx: s.netTxBytes }
        }
    }

    public static func hostSeries(_ points: [MetricsPoint]) -> [SeriesPoint] {
        points.flatMap { p -> [SeriesPoint] in
            [("CPU", p.hostCpuPercent), ("Memory", p.hostMemoryPercent), ("Disk", p.hostDiskPercent)]
                .compactMap { name, v in v.map { SeriesPoint(date: p.date, value: $0, series: name) } }
        }
    }

    private static func sample(_ p: MetricsPoint, _ vm: String) -> VmMetricsSample? { p.vmMetrics?.first { $0.name == vm } }

    /// Pairs of consecutive samples for one VM with the elapsed seconds between them.
    private static func pairs(_ points: [MetricsPoint], _ vm: String) -> [(VmMetricsSample, VmMetricsSample, Date, Double)] {
        let sorted = points.sorted { $0.timestampMs < $1.timestampMs }
        var out: [(VmMetricsSample, VmMetricsSample, Date, Double)] = []
        for i in sorted.indices.dropFirst() {
            guard let a = sample(sorted[i - 1], vm), let b = sample(sorted[i], vm) else { continue }
            let dt = (sorted[i].timestampMs - sorted[i - 1].timestampMs) / 1000
            if dt > 0 { out.append((a, b, sorted[i].date, dt)) }
        }
        return out
    }

    /// Percent of the VM's allotted CPU (all vCPUs = 100%).
    public static func vmCPU(_ points: [MetricsPoint], vm: String) -> [SeriesPoint] {
        pairs(points, vm).compactMap { a, b, date, dt in
            guard let x = a.cpuTimeNs, let y = b.cpuTimeNs, y >= x else { return nil }
            let cores = Double(max(b.vcpus ?? 1, 1))
            return SeriesPoint(date: date, value: min((y - x) / (dt * 1e9 * cores) * 100, 100 * 1.0), series: "CPU")
        }
    }

    public static func vmMemory(_ points: [MetricsPoint], vm: String) -> [SeriesPoint] {
        points.sorted { $0.timestampMs < $1.timestampMs }.compactMap { p in
            sample(p, vm)?.memoryUsedMb.map { SeriesPoint(date: p.date, value: $0, series: "Used (MiB)") }
        }
    }

    /// Bytes per second for each requested counter.
    public static func vmRates(_ points: [MetricsPoint], vm: String, _ counters: [Counter]) -> [SeriesPoint] {
        let pr = pairs(points, vm)
        return counters.flatMap { c in
            pr.compactMap { a, b, date, dt -> SeriesPoint? in
                guard let x = c.value(a), let y = c.value(b), y >= x else { return nil }
                return SeriesPoint(date: date, value: (y - x) / dt, series: c.rawValue)
            }
        }
    }
}

public extension MachinaClient {
    func metricsHistory(limit: Int = 120) async throws -> [MetricsPoint] {
        let v = try await call(.get("/api/v1/metrics/history", query: [URLQueryItem(name: "limit", value: String(limit))]))
        let pts = v["points"] ?? .array([])
        return try JSONDecoder().decode([MetricsPoint].self, from: try JSONEncoder().encode(pts))
    }

    /// Tail of the VM's libvirt/QEMU log (`GET /api/v1/vms/{name}/logs`).
    func vmLog(_ vm: VmInfo) async throws -> String {
        let v = try await call(Endpoint("GET", "/api/v1/vms/\(Self.seg(vm.name))/logs", query: vm.scopeQuery))
        return v["content"]?.text ?? ""
    }
}
