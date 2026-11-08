import Foundation

/// AI insight pages (controller `api/ai.rs`): Black Box root cause, performance autopilot, migration oracle and the
/// adaptive decision, fleet summary / heatmap / power, and noisy neighbours. Everything is observe or recommend only,
/// except the two single-VM executes at the bottom (admin only, previewed, typed confirmation).
public enum AIInsightCopy {
    /// docs/claims.md C28 and C29.
    public static let unverified = "Unit-tested only. These models have never been run against a real incident, migration or fleet, and every figure is a model output, not a measurement. Treat results as hints to check."
    public static let blackBox = "Black Box root cause (claim C28): the ranking comes from a frozen bpfd capture and is unit-tested only. A score is evidence strength from 0 to 100, not a probability."
    public static let planning = "Performance autopilot, migration oracle and adaptive decision (claim C29): recommendations only, unit-tested only, never run on a real fleet. Nothing here changes a VM or host."
}

public struct RcaHypothesis: Equatable, Sendable, Identifiable {
    public var id: String, title: String
    public var score: Double
    public var evidence: [String], actions: [String]
    public init(json v: JSONValue) {
        id = v["id"]?.text ?? ""; title = v["title"]?.text ?? ""
        score = v["score"]?.double ?? 0
        evidence = (v["evidence"]?.array ?? []).map(\.text)
        actions = (v["suggested_actions"]?.array ?? []).map(\.text)
    }
}

/// `GET /api/v1/ai/incidents/blackbox?vm_name=` (operator).
public struct BlackBoxReport: Equatable, Sendable {
    public var vm: String, hostname: String, incidentID: String, triggeredAt: String, rootCause: String
    public var frozen: Bool, deterministic: Bool
    /// 0...1: how far the top hypothesis is ahead of the rest. Not a failure probability.
    public var confidence: Double
    /// Highest score first (ties keep the server's order).
    public var hypotheses: [RcaHypothesis]
    public var evidence: [String], actions: [String]
    public var events: Int, severeEvents: Int
    public var topics: [TopicCount]
    public struct TopicCount: Equatable, Sendable { public var name: String; public var count: Int }

    public init(json v: JSONValue) {
        vm = v["vm"]?.text ?? ""; hostname = v["hostname"]?.text ?? ""
        incidentID = v["incident_id"]?.text ?? ""; triggeredAt = v["triggered_at"]?.text ?? ""
        rootCause = v["root_cause"]?.text ?? ""
        frozen = v["frozen"]?.double == 1; deterministic = v["deterministic"]?.double == 1
        confidence = min(1, max(0, v["confidence"]?.double ?? 0))
        let h = (v["hypotheses"]?.array ?? []).map(RcaHypothesis.init(json:))
        hypotheses = h.enumerated().sorted { $0.element.score != $1.element.score ? $0.element.score > $1.element.score : $0.offset < $1.offset }.map(\.element)
        evidence = (v["evidence"]?.array ?? []).map(\.text)
        actions = (v["suggested_actions"]?.array ?? []).map(\.text)
        events = Int(v["stats"]?["events"]?.double ?? 0); severeEvents = Int(v["stats"]?["severe_events"]?.double ?? 0)
        topics = (v["stats"]?["topics"]?.object ?? [:]).map { TopicCount(name: $0.key, count: Int($0.value.double ?? 0)) }
            .sorted { $0.count != $1.count ? $0.count > $1.count : $0.name < $1.name }
    }
    public var hasCapture: Bool { frozen && !hypotheses.isEmpty }
    /// Bar share of a hypothesis relative to the top score, 0...1.
    public func share(_ h: RcaHypothesis) -> Double { (hypotheses.first?.score ?? 0) > 0 ? h.score / hypotheses[0].score : 0 }
}

public struct PerfEvidence: Equatable, Sendable, Identifiable {
    public var id: String
    public var source: String, metric: String, unit: String, summary: String
    public var value: Double, threshold: Double
    public init(index: Int, json v: JSONValue) {
        id = "\(index)"; source = v["source"]?.text ?? ""; metric = v["metric"]?.text ?? ""; unit = v["unit"]?.text ?? ""
        summary = v["summary"]?.text ?? ""; value = v["value"]?.double ?? 0; threshold = v["threshold"]?.double ?? 0
    }
}

public struct PerfAction: Equatable, Sendable, Identifiable {
    public var id: String, title: String, actionType: String, expectedBenefit: String, risk: String
    public var priority: Int
    public var score: Double
    public var requiresApproval: Bool, reversible: Bool
    public var evidence: [PerfEvidence]
    public init(json v: JSONValue) {
        id = v["id"]?.text ?? ""; title = v["title"]?.text ?? ""; actionType = v["action_type"]?.text ?? ""
        expectedBenefit = v["expected_benefit"]?.text ?? ""; risk = v["risk"]?.text ?? ""
        priority = Int(v["priority"]?.double ?? 0); score = v["score"]?.double ?? 0
        requiresApproval = v["requires_approval"]?.double == 1; reversible = v["reversible"]?.double == 1
        evidence = (v["evidence"]?.array ?? []).enumerated().map { PerfEvidence(index: $0, json: $1) }
    }
}

/// `GET /api/v1/ai/performance-autopilot?vm_id=` (operator): one VM's pressure and ranked plan.
public struct PerfReport: Equatable, Sendable {
    public var vmID: String, vmName: String, hostname: String, generatedAt: String, summary: String
    public var healthScore: Double
    /// Pressure per resource, 0...100, highest first.
    public var pressures: [Pressure]
    public struct Pressure: Equatable, Sendable, Identifiable { public var id: String { name }; public var name: String; public var value: Double }
    /// Highest priority (1...100, 100 most urgent) first, then score.
    public var actions: [PerfAction]
    public init(json v: JSONValue) {
        vmID = v["vm_id"]?.text ?? ""; vmName = v["vm_name"]?.text ?? ""; hostname = v["hostname"]?.text ?? ""
        generatedAt = v["generated_at"]?.text ?? ""; summary = v["summary"]?.text ?? ""
        let h = v["health"]
        healthScore = h?["score"]?.double ?? 0
        pressures = [("CPU", "cpu_pressure"), ("Scheduler", "scheduler_pressure"), ("Storage", "storage_pressure"),
                     ("Memory", "memory_pressure"), ("Network", "network_pressure")]
            .map { Pressure(name: $0.0, value: h?[$0.1]?.double ?? 0) }.sorted { $0.value > $1.value }
        actions = (v["actions"]?.array ?? []).map(PerfAction.init(json:))
            .sorted { $0.priority != $1.priority ? $0.priority > $1.priority : $0.score > $1.score }
    }
}

public struct OracleRound: Equatable, Sendable, Identifiable {
    public var id: Int
    public var remainingMiB: Double, seconds: Double, dirtiedMiB: Double
    public init(json v: JSONValue) {
        id = Int(v["round"]?.double ?? 0); remainingMiB = v["remaining_mib"]?.double ?? 0
        seconds = v["seconds"]?.double ?? 0; dirtiedMiB = v["dirtied_mib"]?.double ?? 0
    }
}

/// `POST /api/v1/vms/{id}/migration-oracle` (operator): will pre-copy converge, and is the destination worth it.
public struct OracleReport: Equatable, Sendable {
    public enum Verdict: String, Sendable {
        case converges, marginal, wontConverge, unknown
        public var label: String { switch self { case .converges: "Converges"; case .marginal: "Marginal"; case .wontConverge: "Will not converge"; case .unknown: "Unknown" } }
    }
    public var vmName: String, source: String, destination: String, convergence: String
    public var precheckOK: Bool, live: Bool
    public var score: Double, totalSeconds: Double, downtimeMs: Double, transferredGiB: Double
    public var dirtyRate: Double, bandwidth: Double, minimumBandwidthMbps: Double
    public var benefit: Double, cpuImprovementPct: Double, headroomMiB: Int
    public var rounds: [OracleRound]
    public var risks: [String], evidence: [String], recommendations: [String]
    public init(json v: JSONValue) {
        vmName = v["vm_name"]?.text ?? ""; source = v["source_hostname"]?.text ?? ""; destination = v["dest_hostname"]?.text ?? ""
        convergence = v["convergence"]?.text ?? ""; precheckOK = v["precheck_ok"]?.double == 1; live = v["live"]?.double == 1
        score = v["convergence_score"]?.double ?? 0; totalSeconds = v["estimated_total_seconds"]?.double ?? 0
        downtimeMs = v["estimated_downtime_ms"]?.double ?? 0; transferredGiB = v["estimated_transferred_gib"]?.double ?? 0
        dirtyRate = v["dirty_rate_mib_s"]?.double ?? 0; bandwidth = v["effective_bandwidth_mib_s"]?.double ?? 0
        minimumBandwidthMbps = v["minimum_bandwidth_mbps"]?.double ?? 0
        benefit = v["destination_benefit_score"]?.double ?? 0; cpuImprovementPct = v["predicted_cpu_improvement_pct"]?.double ?? 0
        headroomMiB = Int(v["memory_headroom_after_mib"]?.double ?? 0)
        rounds = (v["rounds"]?.array ?? []).map(OracleRound.init(json:))
        risks = (v["risks"]?.array ?? []).map(\.text); evidence = (v["evidence"]?.array ?? []).map(\.text)
        recommendations = (v["recommendations"]?.array ?? []).map(\.text)
    }
    /// The server's convergence word is free text; map it conservatively (negative words win).
    public var verdict: Verdict {
        let c = convergence.lowercased()
        if c.contains("not") || c.contains("diverg") || c.contains("never") || c.contains("fail") || c.contains("unlikely") { return .wontConverge }
        if c.contains("marginal") || c.contains("slow") || c.contains("risky") || c.contains("borderline") { return .marginal }
        if c.contains("converg") || c.contains("likely") || c.contains("good") { return .converges }
        return .unknown
    }
    /// Safe to attempt only when the verdict is positive and the fail-closed pre-check passed.
    public var looksSafe: Bool { precheckOK && verdict == .converges }
}

public struct OracleRequest: Equatable, Sendable {
    public var destHostID: String
    public var live = true
    public var dirtyRateMiBs: Double?, bandwidthMbps: Double?, maxDowntimeMs: Double?
    public init(destHostID: String) { self.destHostID = destHostID }
    public func body() -> JSONValue {
        var o: [String: JSONValue] = ["dest_host_id": .string(destHostID), "live": .bool(live)]
        if let d = dirtyRateMiBs { o["dirty_rate_mib_s"] = .number(d) }
        if let b = bandwidthMbps { o["bandwidth_mbps"] = .number(b) }
        if let m = maxDowntimeMs { o["max_downtime_ms"] = .number(m) }
        return .object(o)
    }
    public func validate() -> String? {
        if UUID(uuidString: destHostID) == nil { return "Pick a destination host." }
        for (n, v) in [("Dirty rate", dirtyRateMiBs), ("Bandwidth", bandwidthMbps), ("Max downtime", maxDowntimeMs)] { if let v, v <= 0 { return "\(n) must be positive." } }
        return nil
    }
}

/// `POST /api/v1/migrations/adaptive/decision` body: one telemetry sample of a running migration.
public struct AdaptiveTelemetry: Equatable, Sendable {
    public var elapsedSecs = 60.0, iteration = 3.0, remainingMiB = 2048.0, transferredMiB = 8192.0, dirtyRate = 200.0, bandwidth = 400.0
    public var guestCpu = 40.0, sourceCpu = 50.0, destCpu = 30.0, packetLoss = 0.0, currentBandwidth = 400.0
    public var compression = false, postcopy = false
    public var throttle = 0.0, stalledRounds = 0.0
    public init() {}
    public func body() -> JSONValue {
        .object(["elapsed_secs": .number(elapsedSecs.rounded()), "iteration": .number(iteration.rounded()), "remaining_mib": .number(remainingMiB),
                 "transferred_mib": .number(transferredMiB), "dirty_rate_mib_s": .number(dirtyRate), "effective_bandwidth_mib_s": .number(bandwidth),
                 "guest_cpu_pct": .number(guestCpu), "source_cpu_pct": .number(sourceCpu), "destination_cpu_pct": .number(destCpu),
                 "packet_loss_pct": .number(packetLoss), "current_bandwidth_mib_s": .number(currentBandwidth.rounded()),
                 "compression_enabled": .bool(compression), "postcopy_active": .bool(postcopy),
                 "vcpu_throttle_pct": .number(throttle.rounded()), "stalled_rounds": .number(stalledRounds.rounded())])
    }
    public func validate() -> String? {
        let nonNeg = [elapsedSecs, iteration, remainingMiB, transferredMiB, dirtyRate, bandwidth, currentBandwidth, stalledRounds]
        if nonNeg.contains(where: { $0 < 0 }) { return "Values cannot be negative." }
        if [guestCpu, sourceCpu, destCpu, packetLoss, throttle].contains(where: { $0 < 0 || $0 > 100 }) { return "Percentages must be 0 to 100." }
        return nil
    }
}

public struct AdaptiveDecision: Equatable, Sendable {
    public var kind: String, reason: String
    public var bandwidth: Double, targetDowntimeMs: Double, throttlePct: Double, risk: Double
    /// nil when the server sent null (an infinite ratio or downtime serialises as null): unbounded, not zero.
    public var convergenceRatio: Double?, estimatedDowntimeMs: Double?
    public var compression: Bool, postcopy: Bool
    public init(json v: JSONValue) {
        kind = v["kind"]?.text ?? ""; reason = v["reason"]?.text ?? ""
        bandwidth = v["bandwidth_mib_s"]?.double ?? 0; targetDowntimeMs = v["target_downtime_ms"]?.double ?? 0
        throttlePct = v["vcpu_throttle_pct"]?.double ?? 0; risk = v["risk_score"]?.double ?? 0
        convergenceRatio = v["convergence_ratio"]?.double; estimatedDowntimeMs = v["estimated_downtime_ms"]?.double
        compression = v["compression"]?.double == 1; postcopy = v["postcopy"]?.double == 1
    }
    public var title: String {
        switch kind {
        case "hold": "Hold"; case "increase_bandwidth": "Increase bandwidth"; case "enable_compression": "Enable compression"
        case "throttle_vcpu": "Throttle vCPUs"; case "raise_downtime": "Raise allowed downtime"; case "switch_postcopy": "Switch to post-copy"
        case "abort": "Abort"; case "ready_for_stop_and_copy": "Ready for stop-and-copy"; default: kind
        }
    }
}

public struct FleetSliceInfo: Equatable, Sendable, Identifiable {
    public var id: String { label }
    public var label: String, risk: String
    public var reachable: Bool
    public var vmCount: Int, headroomMiB: Int
    public var monthlyUSD: Double
}

public struct FleetSummaryInfo: Equatable, Sendable {
    public var clusters: [FleetSliceInfo]
    public var aggregateUSD: Double, vmCount: Int, peers: Int, reachablePeers: Int
    public init(json v: JSONValue) {
        clusters = (v["clusters"]?.array ?? []).map {
            FleetSliceInfo(label: $0["label"]?.text ?? "", risk: $0["security_risk_level"]?.text ?? "", reachable: $0["reachable"]?.double == 1,
                           vmCount: Int($0["vm_count"]?.double ?? 0), headroomMiB: Int($0["memory_headroom_mib"]?.double ?? 0), monthlyUSD: $0["estimated_monthly_usd"]?.double ?? 0)
        }
        aggregateUSD = v["aggregate_monthly_usd"]?.double ?? 0; vmCount = Int(v["aggregate_vm_count"]?.double ?? 0)
        peers = Int(v["peer_count"]?.double ?? 0); reachablePeers = Int(v["reachable_peers"]?.double ?? 0)
    }
}

public struct HeatCell: Equatable, Sendable, Identifiable {
    public var id: String, hostname: String, classification: String
    public var cpu: Double, memory: Double
    public var vmCount: Int
}

public struct FleetHeat: Equatable, Sendable {
    /// Hottest host (by the larger of CPU and memory) first.
    public var hosts: [HeatCell]
    public var hotspots: [String], cold: [String], wasteful: [String]
    public init(json v: JSONValue) {
        hosts = (v["hosts"]?.array ?? []).map {
            HeatCell(id: $0["host_id"]?.text ?? "", hostname: $0["hostname"]?.text ?? "", classification: $0["classification"]?.text ?? "",
                     cpu: $0["cpu_percent"]?.double ?? 0, memory: $0["memory_percent"]?.double ?? 0, vmCount: Int($0["vm_count"]?.double ?? 0))
        }.sorted { max($0.cpu, $0.memory) > max($1.cpu, $1.memory) }
        hotspots = (v["hotspots"]?.array ?? []).map(\.text); cold = (v["cold_hosts"]?.array ?? []).map(\.text)
        wasteful = (v["power_waste_hosts"]?.array ?? []).map(\.text)
    }
}

public struct PowerReport: Equatable, Sendable {
    public struct Item: Equatable, Sendable, Identifiable {
        public var id: String { host + action }
        public var host: String, action: String, reason: String
        public var savingsUSD: Double
    }
    /// Biggest saving first.
    public var items: [Item]
    public var totalUSD: Double, summary: String
    public init(json v: JSONValue) {
        items = (v["optimizations"]?.array ?? []).map {
            Item(host: $0["host"]?.text ?? "", action: $0["action"]?.text ?? "", reason: $0["reason"]?.text ?? "", savingsUSD: $0["estimated_savings_usd_month"]?.double ?? 0)
        }.sorted { $0.savingsUSD > $1.savingsUSD }
        totalUSD = v["total_savings_usd_month"]?.double ?? 0; summary = v["summary"]?.text ?? ""
    }
}

public struct NoisyEdge: Equatable, Sendable, Identifiable {
    public var id: String { "\(aggressor)>\(victim)@\(hostname)" }
    public var aggressor: String, victim: String, hostname: String
    public var score: Double, confidence: Double, impactPct: Double
    public var resources: [String], evidence: [String], recommendations: [String]
    init(json v: JSONValue) {
        aggressor = v["aggressor"]?.text ?? ""; victim = v["victim"]?.text ?? ""; hostname = v["hostname"]?.text ?? ""
        score = v["score"]?.double ?? 0; confidence = v["confidence"]?.double ?? 0; impactPct = v["estimated_latency_impact_pct"]?.double ?? 0
        resources = (v["resources"]?.array ?? []).map(\.text); evidence = (v["evidence"]?.array ?? []).map(\.text)
        recommendations = (v["recommendations"]?.array ?? []).map(\.text)
    }
}

/// A node placed on one of two columns of the aggressor-to-victim graph (x, y in 0...1).
public struct NoisyNode: Equatable, Sendable, Identifiable {
    public var id: String { name }
    public var name: String
    public var isAggressor: Bool, isVictim: Bool
    public var x: Double, y: Double
}

public struct NoisyReport: Equatable, Sendable {
    public var summary: String, generatedAt: String
    /// Strongest first.
    public var edges: [NoisyEdge]
    public init(json v: JSONValue) {
        summary = v["summary"]?.text ?? ""; generatedAt = v["generated_at"]?.text ?? ""
        edges = (v["top_edges"]?.array ?? []).map(NoisyEdge.init(json:)).sorted { $0.score > $1.score }
    }
    /// Aggressors on the left (x = 0), pure victims on the right (x = 1). A VM that is both sits left, so its
    /// outgoing and incoming edges stay readable. Rows are ordered by the strongest edge touching the VM.
    public var nodes: [NoisyNode] {
        var strength: [String: Double] = [:]
        for e in edges {
            strength[e.aggressor] = max(strength[e.aggressor] ?? 0, e.score)
            strength[e.victim] = max(strength[e.victim] ?? 0, e.score)
        }
        let aggressors = Set(edges.map(\.aggressor)), victims = Set(edges.map(\.victim))
        func column(_ names: [String], x: Double) -> [NoisyNode] {
            let sorted = names.sorted { strength[$0]! != strength[$1]! ? strength[$0]! > strength[$1]! : $0 < $1 }
            return sorted.enumerated().map { i, n in
                NoisyNode(name: n, isAggressor: aggressors.contains(n), isVictim: victims.contains(n), x: x,
                          y: sorted.count == 1 ? 0.5 : Double(i) / Double(sorted.count - 1))
            }
        }
        return column(Array(aggressors), x: 0) + column(Array(victims.subtracting(aggressors)), x: 1)
    }
}

/// Endpoints. VM ids are controller UUIDs; the Black Box trigger is a daemon route (admin) and takes the libvirt name.
public enum AIInsightAPI {
    public static func blackBox(vm: String, hostID: String? = nil) -> Endpoint {
        var q = [URLQueryItem(name: "vm_name", value: vm)]
        if let h = hostID, !h.isEmpty { q.append(URLQueryItem(name: "host_id", value: h)) }
        return .controllerGet("/api/v1/ai/incidents/blackbox", query: q)
    }
    public static func blackBoxTrigger(vm: String, postSecs: Int = 15, reason: String) -> Endpoint {
        Endpoint("POST", "/api/v1/bpf/blackbox/\(MachinaClient.seg(vm))/trigger",
                 body: .object(["post_secs": .number(Double(min(300, max(1, postSecs)))), "reason": .string(reason)]))
    }
    public static func performance(vmID: String) -> Endpoint {
        .controllerGet("/api/v1/ai/performance-autopilot", query: [URLQueryItem(name: "vm_id", value: vmID), URLQueryItem(name: "limit", value: "20")])
    }
    public static func autopilotPropose(vmID: String) -> Endpoint {
        .controllerGet("/api/v1/ai/autopilot/propose", query: [URLQueryItem(name: "vm_id", value: vmID)])
    }
    public static func oracle(vmID: String, _ r: OracleRequest) -> Endpoint {
        Endpoint("POST", "/api/v1/vms/\(MachinaClient.seg(vmID))/migration-oracle", body: r.body(), controller: true, timeout: 60)
    }
    public static func decision(_ t: AdaptiveTelemetry) -> Endpoint { Endpoint("POST", "/api/v1/migrations/adaptive/decision", body: t.body(), controller: true) }
    public static let fleetSummary = Endpoint.controllerGet("/api/v1/ai/fleet/summary")
    public static let heatmap = Endpoint.controllerGet("/api/v1/ai/fleet/heatmap")
    public static let power = Endpoint.controllerGet("/api/v1/ai/fleet/power/optimize")
    public static let noisy = Endpoint.controllerGet("/api/v1/ai/interference/noisy-neighbors", query: [URLQueryItem(name: "limit", value: "20")])
    public static func rebalancePropose(maxMoves: Int = 1) -> Endpoint {
        .controllerGet("/api/v1/ai/fleet/rebalance/propose", query: [URLQueryItem(name: "max_moves", value: String(min(20, max(1, maxMoves))))])
    }
}

/// One rebalance move. The execute endpoint takes only `max_moves`, so the app always sends 1 and re-checks the
/// named VM against a fresh dry run first.
public struct RebalanceMove: Equatable, Sendable, Identifiable {
    public var id: String { vmID }
    public var vmID: String, vmName: String, from: String, to: String, reason: String
    public var score: Double
    public init(json v: JSONValue) {
        vmID = v["vm_id"]?.text ?? ""; vmName = v["vm_name"]?.text ?? ""; from = v["from_host"]?.text ?? ""; to = v["to_host"]?.text ?? ""
        reason = v["reason"]?.text ?? ""; score = v["score"]?.double ?? 0
    }
}

/// One `GET /api/v1/ai/autopilot/propose?vm_id=` action (the vocabulary `/ai/autopilot/execute` understands;
/// the performance plan above uses different ids and is advice only).
public struct AutopilotProposal: Equatable, Sendable, Identifiable {
    public var id: String, label: String, review: String, risk: String, actionType: String
    /// The single VM the action names, when `object_ref.vm_id` is one string; nil for bulk or host-level actions.
    public var vmID: String?
    public init(json v: JSONValue) {
        id = v["id"]?.text ?? ""; label = v["label"]?.text ?? ""; review = v["review"]?.text ?? ""; risk = v["risk"]?.text ?? ""
        actionType = v["action_type"]?.text ?? ""
        if case .string(let s)? = v["object_ref"]?["vm_id"] { vmID = s } else { vmID = nil }
    }
    public static func list(_ v: JSONValue) -> [AutopilotProposal] { (v["actions"]?.array ?? []).map(AutopilotProposal.init(json:)) }
}

/// Admin-only executes. Both are one VM, previewed first, confirmed by typing the action name and the VM.
public enum AIExecute {
    /// Autopilot actions the app will send: single-VM, no data loss. `stop_vm`, `remove_stale_vm`, `bulk_*` are never sent.
    public static let autopilotAllowed: Set<String> = ["create_backup", "start_vm", "install_guest_tools"]

    public static func canExecute(role: String?) -> Bool { role == "admin" }

    public static func autopilotBlocker(role: String?, actionType: String, vmID: String) -> String? {
        if !canExecute(role: role) { return "Needs the admin role." }
        if !autopilotAllowed.contains(actionType) { return "\(actionType) is not offered in the app (one VM at a time, and only backup, start or guest tools)." }
        if UUID(uuidString: vmID) == nil { return "No single VM to act on." }
        return nil
    }
    public static func autopilotBody(actionType: String, vmID: String) -> JSONValue {
        .object(["action_type": .string(actionType), "object_ref": .object(["vm_id": .string(vmID)])])
    }
    public static func autopilotExecute(actionType: String, vmID: String) -> Endpoint {
        Endpoint("POST", "/api/v1/ai/autopilot/execute", body: autopilotBody(actionType: actionType, vmID: vmID), controller: true)
    }
    /// Phrase to type: "<action> <vm name>".
    public static func autopilotPhrase(actionType: String, vmName: String) -> String { "\(actionType) \(vmName)" }

    public static func rebalanceBlocker(role: String?) -> String? { canExecute(role: role) ? nil : "Needs the admin role." }
    public static func rebalanceBody(dryRun: Bool) -> JSONValue { .object(["dry_run": .bool(dryRun), "max_moves": .number(1)]) }
    public static func rebalanceExecute(dryRun: Bool) -> Endpoint {
        Endpoint("POST", "/api/v1/ai/fleet/rebalance/execute", body: rebalanceBody(dryRun: dryRun), controller: true, timeout: 60)
    }
    public static func rebalancePhrase(vmName: String) -> String { "rebalance \(vmName)" }
    public static func rebalanceMoves(_ v: JSONValue) -> [RebalanceMove] { (v["moves"]?.array ?? []).map(RebalanceMove.init(json:)) }
    /// Execute only if a fresh dry run still names exactly the VM the user confirmed.
    public static func rebalanceStillMatches(confirmed: String, fresh: [RebalanceMove]) -> Bool { fresh.count == 1 && fresh[0].vmID == confirmed }
    public static func taskIDs(_ v: JSONValue) -> [String] { (v["task_ids"]?.array ?? []).map(\.text) }
}
