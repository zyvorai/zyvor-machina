import Testing
import Foundation
@testable import MachinaKit

private final class AIMock: URLProtocol, @unchecked Sendable {
    nonisolated(unsafe) static var handler: (@Sendable (URLRequest) -> (Int, String))?
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        let (code, body) = Self.handler?(request) ?? (500, "")
        client?.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: code, httpVersion: nil, headerFields: nil)!, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(body.utf8)); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

private func j(_ s: String) -> JSONValue { try! JSONDecoder().decode(JSONValue.self, from: Data(s.utf8)) }
private let vm1 = "11111111-1111-1111-1111-111111111111"

private let rcaJSON = """
{"vm":"web-1","host_id":"h","hostname":"kvm1","incident_id":"i1","triggered_at":"2026-01-01T10:00:00Z","frozen":true,"deterministic":true,
"root_cause":"Storage stall","confidence":0.62,
"hypotheses":[{"id":"net","title":"Network drops","score":30,"evidence":["drops"],"suggested_actions":[]},
{"id":"storage","title":"Storage stall","score":80,"evidence":["block p99 900ms","virtio timeouts"],"suggested_actions":["move disk"]},
{"id":"cpu","title":"vCPU starvation","score":80,"evidence":[],"suggested_actions":[]}],
"evidence":["e"],"suggested_actions":["a"],"stats":{"events":120,"severe_events":4,"topics":{"block":90,"net":30}}}
"""

@Suite(.serialized) struct AIInsightTests {
    @Test func blackBoxRanksHypothesesAndStats() {
        let r = BlackBoxReport(json: j(rcaJSON))
        #expect(r.hypotheses.map(\.id) == ["storage", "cpu", "net"])      // score desc, tie keeps server order
        #expect(r.hasCapture); #expect(r.confidence == 0.62)
        #expect(r.share(r.hypotheses[2]) == 30.0 / 80.0)
        #expect(r.topics.first == .init(name: "block", count: 90)); #expect(r.severeEvents == 4)
        let empty = BlackBoxReport(json: j(#"{"vm":"x","frozen":false,"hypotheses":[],"confidence":4}"#))
        #expect(!empty.hasCapture); #expect(empty.confidence == 1)
    }

    @Test func autopilotPlanOrderedByPriority() {
        let v = j("""
        {"vm_id":"\(vm1)","vm_name":"web-1","health":{"score":55,"cpu_pressure":10,"scheduler_pressure":70,"storage_pressure":90,"memory_pressure":5,"network_pressure":0},
        "actions":[{"id":"a","title":"A","action_type":"x","priority":40,"score":40.2,"expected_benefit":"-10% latency","risk":"low","requires_approval":false,"reversible":true,"object_ref":{},"evidence":[{"source":"bpf","metric":"p99","value":9,"unit":"ms","threshold":5,"summary":"s"}]},
        {"id":"b","title":"B","action_type":"y","priority":90,"score":90,"expected_benefit":"big","risk":"high","requires_approval":true,"reversible":false,"object_ref":{},"evidence":[]}],"summary":"s"}
        """)
        let r = PerfReport(json: v)
        #expect(r.actions.map(\.id) == ["b", "a"])
        #expect(r.pressures.first?.name == "Storage"); #expect(r.pressures.last?.name == "Network")
        #expect(r.actions[1].evidence.first?.threshold == 5); #expect(r.actions[0].requiresApproval)
    }

    @Test func oracleVerdictIsConservative() {
        func rep(_ c: String, pre: Bool = true) -> OracleReport {
            OracleReport(json: j("{\"convergence\":\"\(c)\",\"precheck_ok\":\(pre),\"rounds\":[{\"round\":1,\"remaining_mib\":100,\"seconds\":2,\"dirtied_mib\":10}]}"))
        }
        #expect(rep("converges").verdict == .converges); #expect(rep("converges").looksSafe)
        #expect(!rep("converges", pre: false).looksSafe)
        #expect(rep("will not converge").verdict == .wontConverge)
        #expect(rep("unlikely to converge").verdict == .wontConverge)
        #expect(rep("marginal").verdict == .marginal); #expect(rep("???").verdict == .unknown)
        #expect(rep("converges").rounds.count == 1)
    }

    @Test func oracleRequestValidationAndBody() {
        var r = OracleRequest(destHostID: "nope"); #expect(r.validate() != nil)
        r.destHostID = vm1; #expect(r.validate() == nil)
        r.bandwidthMbps = 0; #expect(r.validate()?.contains("Bandwidth") == true)
        r.bandwidthMbps = 10_000; r.maxDowntimeMs = 300
        let b = r.body(); #expect(b["dest_host_id"] == .string(vm1)); #expect(b["bandwidth_mbps"] == .number(10_000)); #expect(b["dirty_rate_mib_s"] == nil)
    }

    @Test func adaptiveTelemetryAndNullDecision() {
        var t = AdaptiveTelemetry(); #expect(t.validate() == nil)
        t.packetLoss = 120; #expect(t.validate() != nil)
        t.packetLoss = 0; t.remainingMiB = -1; #expect(t.validate() != nil)
        #expect(AdaptiveTelemetry().body()["stalled_rounds"] == .number(0))
        let d = AdaptiveDecision(json: j(#"{"kind":"switch_postcopy","reason":"stalled","bandwidth_mib_s":500,"target_downtime_ms":300,"compression":false,"postcopy":true,"vcpu_throttle_pct":0,"convergence_ratio":null,"estimated_downtime_ms":null,"risk_score":0.7}"#))
        #expect(d.title == "Switch to post-copy"); #expect(d.convergenceRatio == nil); #expect(d.postcopy)
    }

    @Test func fleetPagesSortAndSum() {
        let h = FleetHeat(json: j(#"{"hosts":[{"host_id":"a","hostname":"a","cpu_percent":20,"memory_percent":30,"vm_count":1,"classification":"normal"},{"host_id":"b","hostname":"b","cpu_percent":10,"memory_percent":95,"vm_count":9,"classification":"hot"}],"hotspots":["b"],"cold_hosts":[],"power_waste_hosts":["a"]}"#))
        #expect(h.hosts.map(\.hostname) == ["b", "a"]); #expect(h.hotspots == ["b"])
        let p = PowerReport(json: j(#"{"optimizations":[{"host":"a","action":"park","estimated_savings_usd_month":10,"reason":"r"},{"host":"c","action":"park","estimated_savings_usd_month":40,"reason":"r"}],"total_savings_usd_month":50,"summary":"s"}"#))
        #expect(p.items.map(\.host) == ["c", "a"]); #expect(p.totalUSD == 50)
        let s = FleetSummaryInfo(json: j(#"{"clusters":[{"label":"local","reachable":true,"vm_count":3,"estimated_monthly_usd":12.5,"memory_headroom_mib":1024,"security_risk_level":"low"}],"aggregate_monthly_usd":12.5,"aggregate_vm_count":3,"peer_count":1,"reachable_peers":0}"#))
        #expect(s.clusters.first?.vmCount == 3); #expect(s.reachablePeers == 0)
    }

    @Test func noisyGraphLayout() {
        let r = NoisyReport(json: j(#"{"summary":"s","top_edges":[{"aggressor":"A","victim":"B","hostname":"h","score":10,"confidence":0.5,"estimated_latency_impact_pct":5,"resources":["cpu"],"evidence":[],"recommendations":[]},{"aggressor":"B","victim":"C","hostname":"h","score":60,"confidence":0.9,"estimated_latency_impact_pct":20,"resources":[],"evidence":[],"recommendations":[]},{"aggressor":"B","victim":"D","hostname":"h","score":30,"confidence":0.4,"estimated_latency_impact_pct":1,"resources":[],"evidence":[],"recommendations":[]}]}"#))
        #expect(r.edges.first?.score == 60)
        let n = r.nodes
        let left = n.filter { $0.x == 0 }.map(\.name), right = n.filter { $0.x == 1 }.map(\.name)
        #expect(left == ["B", "A"])             // B is both, kept on the left; B has the strongest edge
        #expect(right == ["C", "D"])
        #expect(n.first { $0.name == "B" }?.isVictim == true)
        #expect(NoisyReport(json: .object([:])).nodes.isEmpty)
    }

    @Test func executeIsAdminOnlyAndSingleVM() {
        #expect(AIExecute.autopilotBlocker(role: "operator", actionType: "create_backup", vmID: vm1) == "Needs the admin role.")
        #expect(AIExecute.autopilotBlocker(role: nil, actionType: "create_backup", vmID: vm1) != nil)
        #expect(AIExecute.autopilotBlocker(role: "admin", actionType: "bulk_backup", vmID: vm1) != nil)
        #expect(AIExecute.autopilotBlocker(role: "admin", actionType: "remove_stale_vm", vmID: vm1) != nil)
        #expect(AIExecute.autopilotBlocker(role: "admin", actionType: "create_backup", vmID: "all") != nil)
        #expect(AIExecute.autopilotBlocker(role: "admin", actionType: "create_backup", vmID: vm1) == nil)
        #expect(AIExecute.rebalanceBlocker(role: "operator") != nil); #expect(AIExecute.rebalanceBlocker(role: "admin") == nil)
        #expect(AIExecute.rebalanceBody(dryRun: false)["max_moves"] == .number(1))
        #expect(AIExecute.rebalanceBody(dryRun: true)["dry_run"] == .bool(true))
        #expect(AIExecute.autopilotPhrase(actionType: "start_vm", vmName: "web-1") == "start_vm web-1")
        let m = RebalanceMove(json: j(#"{"vm_id":"v","vm_name":"n","from_host":"a","to_host":"b","reason":"r","score":1}"#))
        #expect(AIExecute.rebalanceStillMatches(confirmed: "v", fresh: [m]))
        #expect(!AIExecute.rebalanceStillMatches(confirmed: "other", fresh: [m]))
        #expect(!AIExecute.rebalanceStillMatches(confirmed: "v", fresh: [m, m]))
        let p = AutopilotProposal(json: j(#"{"id":"1","label":"L","review":"r","risk":"Low","action_type":"create_backup","object_ref":{"vm_ids":["a","b"]}}"#))
        #expect(p.vmID == nil)   // bulk reference: nothing to execute from the app
    }

    @Test func endpointsAndWireFormat() async throws {
        let cfg = URLSessionConfiguration.ephemeral; cfg.protocolClasses = [AIMock.self]
        let c = MachinaClient(server: ServerProfile(name: "t", url: URL(string: "https://h:5092")!), session: URLSession(configuration: cfg))
        AIMock.handler = { req in
            let path = req.url!.path
            if path.hasSuffix("/incidents/blackbox") { return (200, rcaJSON) }
            if path.hasSuffix("/trigger") { return (req.httpMethod == "POST" ? 200 : 405, "{}") }
            return (404, "")
        }
        let r = BlackBoxReport(json: try await c.call(AIInsightAPI.blackBox(vm: "web 1")))
        #expect(r.hypotheses.count == 3)
        _ = try await c.call(AIInsightAPI.blackBoxTrigger(vm: "web 1", postSecs: 9999, reason: "test"))
        #expect(AIInsightAPI.blackBoxTrigger(vm: "x", postSecs: 9999, reason: "r").body?["post_secs"] == .number(300))
        #expect(!AIInsightAPI.blackBoxTrigger(vm: "x", reason: "r").viaController)         // daemon route
        #expect(AIInsightAPI.performance(vmID: vm1).path == "/api/v1/ai/performance-autopilot")
        #expect(AIInsightAPI.oracle(vmID: vm1, OracleRequest(destHostID: vm1)).path == "/api/v1/vms/\(vm1)/migration-oracle")
        let ex = AIExecute.autopilotExecute(actionType: "start_vm", vmID: vm1)
        #expect(ex.method == "POST"); #expect(ex.viaController); #expect(ex.body?["object_ref"]?["vm_id"] == .string(vm1))
        #expect(AIExecute.rebalanceExecute(dryRun: true).resolvedPath == "/api/v1/platform/controller/api/v1/ai/fleet/rebalance/execute")
        #expect(AIInsightCopy.planning.contains("C29")); #expect(AIInsightCopy.blackBox.contains("C28"))
    }
}
