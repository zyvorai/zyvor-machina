import Foundation

/// Chaos experiment authoring (controller `api/chaos.rs`, `engine/chaos.rs`). Game days are experiments with several
/// steps and probes; the controller has no separate game-day object and no dry-run flag for runs, so safety here is:
/// no default targets, a validated spec mirroring the server, a blast-radius summary, and the experiment's name typed to run it.
public enum ChaosLimits {
    public static let maxSteps = 20, maxTargets = 20, maxProbes = 10, maxTotalSecs = 3600
}

public enum ChaosStep: Equatable, Sendable {
    case latency(delayMs: Int, jitterMs: Int, secs: Int)
    case loss(pct: Double, secs: Int)
    case partition(cidrs: [String], peers: [String], secs: Int)
    case disk(readIops: Int, writeIops: Int, secs: Int)
    case kill(recoverSecs: Int)
    case hostFailure(hostID: String)

    public var secs: Int {
        switch self {
        case .latency(_, _, let s), .loss(_, let s), .partition(_, _, let s), .disk(_, _, let s): return s
        case .kill(let r): return r
        case .hostFailure: return 0
        }
    }
    /// True when the step really changes the targets (a host failure is only a placement simulation).
    public var injectsFault: Bool { if case .hostFailure = self { return false }; return true }
    public var label: String {
        switch self {
        case .latency(let d, let j, let s): return j > 0 ? "Latency \(d)±\(j) ms for \(s)s" : "Latency \(d) ms for \(s)s"
        case .loss(let p, let s): return "Packet loss \(p)% for \(s)s"
        case .partition(let c, let p, let s):
            let what = ([c.isEmpty ? nil : c.joined(separator: ", "), p.isEmpty ? nil : "\(p.count) VMs"].compactMap { $0 }).joined(separator: " and ")
            return "Partition from \(what) for \(s)s"
        case .disk(let r, let w, let s): return "Disk \(r)/\(w) IOPS (read/write) for \(s)s"
        case .kill(let r): return "Hard power-off, recover within \(r)s"
        case .hostFailure: return "Host failure (simulation only)"
        }
    }
    public var json: JSONValue {
        func n(_ i: Int) -> JSONValue { .number(Double(i)) }
        switch self {
        case .latency(let d, let j, let s): return .object(["kind": .string("latency"), "delay_ms": n(d), "jitter_ms": n(j), "secs": n(s)])
        case .loss(let p, let s): return .object(["kind": .string("loss"), "loss_pct": .number(p), "secs": n(s)])
        case .partition(let c, let p, let s):
            return .object(["kind": .string("partition"), "cidrs": .array(c.map(JSONValue.string)), "peers": .array(p.map(JSONValue.string)), "secs": n(s)])
        case .disk(let r, let w, let s): return .object(["kind": .string("disk"), "read_iops": n(r), "write_iops": n(w), "secs": n(s)])
        case .kill(let r): return .object(["kind": .string("kill"), "recover_secs": n(r)])
        case .hostFailure(let h): return .object(["kind": .string("host_failure"), "host_id": .string(h)])
        }
    }
    public init?(json v: JSONValue) {
        func i(_ k: String, _ d: Int = 0) -> Int { v[k]?.double.map { Int($0) } ?? d }
        func list(_ k: String) -> [String] { (v[k]?.array ?? []).map(\.text) }
        switch v["kind"]?.text {
        case "latency": self = .latency(delayMs: i("delay_ms"), jitterMs: i("jitter_ms"), secs: i("secs"))
        case "loss": self = .loss(pct: v["loss_pct"]?.double ?? 0, secs: i("secs"))
        case "partition": self = .partition(cidrs: list("cidrs"), peers: list("peers"), secs: i("secs"))
        case "disk": self = .disk(readIops: i("read_iops"), writeIops: i("write_iops"), secs: i("secs"))
        case "kill": self = .kill(recoverSecs: i("recover_secs", 180))
        case "host_failure": self = .hostFailure(hostID: v["host_id"]?.text ?? "")
        default: return nil
        }
    }
}

public struct ChaosDraft: Equatable, Sendable {
    public var name = ""
    public var description = ""
    /// VM ids. Starts empty: an experiment never targets "all VMs" by default.
    public var targets: [String] = []
    public var steps: [ChaosStep] = []
    /// Probes are kept as the server's JSON (tcp / http / vm_running); editing preserves ones the form does not show.
    public var probes: [JSONValue] = []
    public var minSuccessPct = 50.0
    public var windowSecs = 20
    public var baselineSecs = 15
    public var recoverySecs = 15
    public init() {}

    public var totalSecs: Int { baselineSecs + recoverySecs + steps.reduce(0) { $0 + $1.secs } }

    /// Same checks the controller makes (it re-checks; this only avoids a round trip). Nil means valid.
    public func validate() -> String? {
        let n = name.trimmingCharacters(in: .whitespaces)
        if n.isEmpty || n.count > 64 || n.contains(where: { $0.isNewline }) { return "Give the experiment a name (at most 64 characters)." }
        if description.count > 2000 { return "Description is at most 2000 characters." }
        if targets.isEmpty { return "Pick at least one target VM. Nothing is targeted by default." }
        if targets.count > ChaosLimits.maxTargets { return "At most \(ChaosLimits.maxTargets) target VMs." }
        if steps.isEmpty || steps.count > ChaosLimits.maxSteps { return "Add 1 to \(ChaosLimits.maxSteps) steps." }
        if probes.count > ChaosLimits.maxProbes { return "At most \(ChaosLimits.maxProbes) probes." }
        if !(0...100).contains(minSuccessPct) { return "The abort threshold is 0 to 100 percent." }
        if !(4...600).contains(windowSecs) { return "The abort window is 4 to 600 seconds." }
        if baselineSecs < 0 || recoverySecs < 0 || baselineSecs > 600 || recoverySecs > 600 { return "Baseline and recovery are at most 600 seconds." }
        if totalSecs > ChaosLimits.maxTotalSecs { return "An experiment runs at most \(ChaosLimits.maxTotalSecs) seconds (this one: \(totalSecs))." }
        for (k, s) in steps.enumerated() {
            let at = "Step \(k + 1)"
            switch s {
            case .latency(let d, let j, let secs):
                if d <= 0 || d > 10_000 || j < 0 || j > d { return "\(at): delay 1 to 10000 ms, jitter at most the delay." }
                if secs <= 0 || secs > ChaosLimits.maxTotalSecs { return "\(at): duration 1 to \(ChaosLimits.maxTotalSecs) seconds." }
            case .loss(let p, let secs):
                if !(p > 0 && p <= 100) { return "\(at): loss is above 0 and at most 100 percent." }
                if secs <= 0 || secs > ChaosLimits.maxTotalSecs { return "\(at): duration 1 to \(ChaosLimits.maxTotalSecs) seconds." }
            case .partition(let c, let p, let secs):
                if c.isEmpty && p.isEmpty { return "\(at): partition from what? Give CIDRs or peer VMs." }
                if secs <= 0 || secs > ChaosLimits.maxTotalSecs { return "\(at): duration 1 to \(ChaosLimits.maxTotalSecs) seconds." }
            case .disk(let r, let w, let secs):
                if r < 0 || w < 0 || (r == 0 && w == 0) { return "\(at): set a read or write IOPS limit." }
                if secs <= 0 || secs > ChaosLimits.maxTotalSecs { return "\(at): duration 1 to \(ChaosLimits.maxTotalSecs) seconds." }
            case .kill(let r): if !(10...1800).contains(r) { return "\(at): recover within 10 to 1800 seconds." }
            case .hostFailure(let h): if UUID(uuidString: h) == nil { return "\(at): give a host id." }
            }
        }
        return nil
    }

    public func body() throws -> JSONValue {
        if let e = validate() { throw ResourceCreate.Invalid(message: e) }
        return .object([
            "name": .string(name.trimmingCharacters(in: .whitespaces)),
            "description": .string(description),
            "spec": .object([
                "targets": .array(targets.map(JSONValue.string)),
                "steps": .array(steps.map(\.json)),
                "probes": .array(probes),
                "abort": .object(["min_success_pct": .number(minSuccessPct), "window_secs": .number(Double(windowSecs))]),
                "baseline_secs": .number(Double(baselineSecs)),
                "recovery_secs": .number(Double(recoverySecs)),
            ]),
        ])
    }

    /// What the confirmation shows before a create, update or run: the real scope in plain words.
    public func blastRadius(vmNames: [String: String] = [:]) -> String {
        let names = targets.map { vmNames[$0] ?? String($0.prefix(8)) }
        var lines = ["Targets (\(targets.count)): \(names.joined(separator: ", "))"]
        for (i, s) in steps.enumerated() { lines.append("\(i + 1). \(s.label)") }
        lines.append(steps.contains { $0.injectsFault } ? "Injects real faults. Longest run: \(totalSecs)s including \(baselineSecs)s baseline and \(recoverySecs)s recovery."
                                                         : "Simulation only: nothing is changed on the targets.")
        lines.append("Aborts by itself if probe success stays under \(Int(minSuccessPct))% for \(windowSecs)s"
                     + (probes.isEmpty ? " (no probes, so nothing can trigger that: watch the run and abort by hand)." : "."))
        if steps.contains(where: { if case .kill = $0 { return true }; return false }) { lines.append("A kill step powers the targets off hard.") }
        return lines.joined(separator: "\n")
    }

    public init(row: JSONValue) {
        name = row["name"]?.text ?? ""
        description = row["description"]?.text ?? ""
        let spec = row["spec"] ?? .null
        targets = (spec["targets"]?.array ?? []).map(\.text)
        steps = (spec["steps"]?.array ?? []).compactMap(ChaosStep.init(json:))
        probes = spec["probes"]?.array ?? []
        minSuccessPct = spec["abort"]?["min_success_pct"]?.double ?? 50
        windowSecs = spec["abort"]?["window_secs"]?.double.map { Int($0) } ?? 20
        baselineSecs = spec["baseline_secs"]?.double.map { Int($0) } ?? 15
        recoverySecs = spec["recovery_secs"]?.double.map { Int($0) } ?? 15
    }

    public static func tcpProbe(_ target: String) -> JSONValue { .object(["kind": .string("tcp"), "target": .string(target)]) }
    public static func httpProbe(_ url: String, status: Int = 200) -> JSONValue {
        .object(["kind": .string("http"), "url": .string(url), "expect_status": .number(Double(status))])
    }
}

public enum ChaosAPI {
    public static func create(_ d: ChaosDraft) throws -> Endpoint { Endpoint("POST", "/api/v1/chaos/experiments", body: try d.body(), controller: true) }
    public static func update(id: String, _ d: ChaosDraft) throws -> Endpoint {
        Endpoint("PUT", "/api/v1/chaos/experiments/\(MachinaClient.seg(id))", body: try d.body(), controller: true)
    }
    public static func delete(id: String) -> Endpoint { Endpoint("DELETE", "/api/v1/chaos/experiments/\(MachinaClient.seg(id))", controller: true) }
    /// The server wants the experiment's name back as confirmation.
    public static func run(id: String, name: String) -> Endpoint {
        Endpoint("POST", "/api/v1/chaos/experiments/\(MachinaClient.seg(id))/run", body: .object(["confirm": .string(name)]), controller: true)
    }
    public static func abort(runID: String) -> Endpoint {
        Endpoint("POST", "/api/v1/chaos/runs/\(MachinaClient.seg(runID))/abort", body: .object([:]), controller: true)
    }
}
