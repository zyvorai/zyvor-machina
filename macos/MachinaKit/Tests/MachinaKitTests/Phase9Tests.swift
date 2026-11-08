import Testing
import Foundation
@testable import MachinaKit

private func client() -> MachinaClient {
    let cfg = URLSessionConfiguration.ephemeral
    cfg.protocolClasses = [MockProtocol.self]
    return MachinaClient(server: ServerProfile(name: "t", url: URL(string: "https://h:5092")!), session: URLSession(configuration: cfg))
}

private func bodyString(_ req: URLRequest) -> String {
    if let b = req.httpBody { return String(decoding: b, as: UTF8.self) }
    guard let st = req.httpBodyStream else { return "" }
    st.open(); defer { st.close() }
    var d = Data(); var buf = [UInt8](repeating: 0, count: 1024)
    while st.hasBytesAvailable { let n = st.read(&buf, maxLength: 1024); if n <= 0 { break }; d.append(buf, count: n) }
    return String(decoding: d, as: UTF8.self)
}

private func row(_ o: [String: JSONValue]) -> ResourceRow { ResourceRow(id: "r", values: .object(o)) }
private func action(_ page: String, _ title: String) -> ResourceAction { FeatureRegistry.spec(page)!.actions.first { $0.title == title }! }
private let p = "/api/v1/platform/controller"

/// JSON object key order is not stable, so compare the decoded body.
private struct Wire { var method: String; var path: String; var query: String; var body: JSONValue?; var timeout: TimeInterval }

extension ClientTests {
    /// Sends the endpoint through the mock and returns what the server would see.
    fileprivate func wire(_ e: Endpoint) async throws -> Wire {
        let seen = Counter(); let t = Counter()
        MockProtocol.failWith = nil
        MockProtocol.handler = { req in
            seen.add([req.httpMethod ?? "", req.url!.path, req.url!.query ?? "", bodyString(req)].joined(separator: "\u{1}"))
            t.add(String(req.timeoutInterval)); return (200, "{}")
        }
        _ = try await client().call(e)
        let f = seen.all()[0].components(separatedBy: "\u{1}")
        return Wire(method: f[0], path: f[1], query: f[2],
                    body: f[3].isEmpty ? nil : try JSONDecoder().decode(JSONValue.self, from: Data(f[3].utf8)),
                    timeout: TimeInterval(t.all()[0]) ?? 0)
    }

    @Test func phase9RegistryPagesExist() {
        for id in ["ap-proposals", "ap-performance", "ap-history", "chaos-experiments", "chaos-runs", "chaos-faults", "preempt-settings", "preempt-vms",
                   "preempt-hosts", "preempt-events", "topology-nodes", "topology-edges", "topology-warnings", "ai-pending", "ai-agents", "ai-incidents",
                   "ai-incident-timeline", "bm-servers", "bm-profiles", "bpf-mode", "bpf-policies", "bpf-nodeiso"] {
            #expect(FeatureRegistry.spec(id) != nil, "missing \(id)")
        }
        let ids = FeatureRegistry.all.map(\.id)
        #expect(Set(ids).count == ids.count)
        // Within a page, button titles are the identity: they must be unique.
        for s in FeatureRegistry.all { #expect(Set(s.actions.map(\.title)).count == s.actions.count, "\(s.id)") }
    }

    @Test func autopilotExecuteSendsActionTypeAndObjectRef() async throws {
        let a = action("ap-proposals", "Execute")
        let r = row(["id": .string("x"), "action_type": .string("vm.resize"), "object_ref": .object(["vm_id": .string("v1")])])
        let w = try await wire(try #require(a.endpoint(for: r)))
        #expect(w.method == "POST" && w.path == "\(p)/api/v1/ai/autopilot/execute")
        #expect(w.body == .object(["action_type": .string("vm.resize"), "object_ref": .object(["vm_id": .string("v1")])]))
        let safe = action("ap-proposals", "Run safe actions")
        let run = try await wire(try #require(safe.endpoint(for: row([:]))))
        #expect(run.method == "POST" && run.path == "\(p)/api/v1/ai/autopilot/run" && run.body == .object(["max_actions": .number(3)]))
        #expect(safe.rowless)
    }

    @Test func performanceAutopilotFansOutPerRunningVmWithLongTimeout() async throws {
        let spec = try #require(FeatureRegistry.spec("ap-performance"))
        let seen = Counter(); let timeouts = Counter()
        MockProtocol.failWith = nil
        MockProtocol.handler = { req in
            seen.add(req.url!.path + (req.url!.query.map { "?" + $0 } ?? ""))
            if req.url!.path.hasSuffix("/api/v1/vms") {
                return (200, #"[{"id":"a1","name":"web","observed_state":"running"},{"id":"b2","name":"db","observed_state":"stopped"}]"#)
            }
            timeouts.add(String(req.timeoutInterval))
            return (200, #"{"vm_id":"a1","actions":[{"id":"1","title":"Pin vCPUs","priority":1,"score":0.9,"risk":"low"}]}"#)
        }
        let rows = spec.rows(from: try await client().load(spec))
        #expect(seen.all().contains("\(p)/api/v1/ai/performance-autopilot?vm_id=a1"))
        #expect(!seen.all().contains { $0.contains("vm_id=b2") })           // stopped VM skipped
        #expect(rows.count == 1 && rows[0].values["_parent"]?.text == "web" && rows[0].values["title"]?.text == "Pin vCPUs")
        #expect(timeouts.all() == ["90.0"])
        #expect(spec.refreshSecs == 0)                                       // heavy report: manual reload
    }

    @Test func incidentsUseLongTimeoutAndReportTimeoutClearly() async throws {
        let spec = try #require(FeatureRegistry.spec("ai-incidents"))
        #expect(spec.endpoint.resolvedPath == "\(p)/api/v1/ai/incidents/active")
        let w = try await wire(spec.endpoint)
        #expect(w.method == "GET" && w.path == "\(p)/api/v1/ai/incidents/active" && w.timeout == 90)
        let analyze = try await wire(try #require(FeatureRegistry.spec("ai-incident-timeline")).endpoint)
        #expect(analyze.path == "\(p)/api/v1/ai/incidents/analyze" && analyze.query == "hours=24" && analyze.timeout == 90)
        // A timeout becomes a message that says what happened and that nothing changed.
        MockProtocol.failWith = .timedOut
        defer { MockProtocol.failWith = nil }
        do { _ = try await client().call(spec.endpoint); Issue.record("expected a timeout") }
        catch let MachinaError.timedOut(path, secs) {
            #expect(path == "/api/v1/ai/incidents/active" && secs == 90)
            #expect(MachinaError.timedOut(path, secs).localizedDescription.contains("after 90 s"))
        }
        let ack = try #require(action("ai-incidents", "Acknowledge").endpoint(for: row(["id": .string("i-1")])))
        #expect(ack.resolvedPath == "\(p)/api/v1/ai/incidents/i-1/ack" && ack.method == "POST")
    }

    @Test func chaosRunNeedsTheExperimentNameAndSendsItAsConfirm() async throws {
        let a = action("chaos-experiments", "Run")
        let r = row(["id": .string("e-1"), "name": .string("kill db")])
        #expect(a.phrase(for: r) == "kill db")
        #expect(a.destructive && a.confirm?.contains("REAL faults") == true)
        let w = try await wire(try #require(a.endpoint(for: r)))
        #expect(w.method == "POST" && w.path == "\(p)/api/v1/chaos/experiments/e-1/run" && w.body == .object(["confirm": .string("kill db")]))
        let abort = try await wire(try #require(action("chaos-runs", "Abort").endpoint(for: row(["id": .string("run-9")]))))
        #expect(abort.method == "POST" && abort.path == "\(p)/api/v1/chaos/runs/run-9/abort" && abort.body == .object([:]))
        #expect(FeatureRegistry.spec("chaos-faults")?.rowsPath == ["items"])
    }

    @Test func preemptibleRequests() async throws {
        let on = action("fc-instances", "Make preemptible")
        let w = try await wire(try #require(on.endpoint(for: row(["id": .string("v-1")]))))
        #expect(w.method == "PUT" && w.path == "\(p)/api/v1/vms/v-1/preemptible")
        #expect(w.body == .object(["preemptible": .bool(true), "priority": .number(0)]))
        let off = try await wire(try #require(action("preempt-vms", "Make non-preemptible").endpoint(for: row(["id": .string("v-1"), "priority": .number(40)]))))
        #expect(off.body == .object(["preemptible": .bool(false), "priority": .number(40)]))
        let form = try #require(FeatureRegistry.spec("preempt-settings")?.create)
        let body = try form.body(from: ["enabled": "yes", "reserve_pct": "15"])
        #expect(body == .object(["enabled": .bool(true), "reserve_pct": .number(15)]))
        #expect(throws: ResourceCreate.Invalid.self) { try form.body(from: ["enabled": "yes", "reserve_pct": "91"]) }   // server max is 90
        var e = form.endpoint; e.body = body
        let w2 = try await wire(e)
        #expect(w2.method == "PUT" && w2.path == "\(p)/api/v1/preemption/settings" && w2.body == body)
    }

    @Test func topologyReadsOneEndpointThreeWays() {
        for (id, key) in [("topology-nodes", "nodes"), ("topology-edges", "edges"), ("topology-warnings", "warnings")] {
            let s = FeatureRegistry.spec(id)!
            #expect(s.endpoint.resolvedPath == "\(p)/api/v1/topology" && s.rowsPath == [key])
        }
    }

    @Test func aiOperatorChatAndApprovals() async throws {
        let form = try #require(FeatureRegistry.spec("ai-agents")?.create)
        #expect(form.timeout == 120 && form.resultKey == "reply")
        var e = form.endpoint; e.body = try form.body(from: ["message": "why is db slow?", "agent": "auto"])
        let w = try await wire(e)
        #expect(w.method == "POST" && w.path == "\(p)/api/v1/ai/zyra/chat" && w.timeout == 120)
        #expect(w.body == .object(["message": .string("why is db slow?"), "agent": .string("auto")]))
        let ok = try await wire(try #require(action("ai-pending", "Approve and run").endpoint(for: row(["id": .string("a-1")]))))
        #expect(ok.method == "POST" && ok.path == "\(p)/api/v1/ai/actions/a-1/execute")
    }

    @Test func bareMetalPowerNeedsAdminAndTypedHostname() async throws {
        let r = row(["id": .string("s-1"), "hostname": .string("node 7")])
        for (title, act) in [("Power on", "on"), ("Power off", "off"), ("Power cycle", "cycle"), ("Hard reset", "reset")] {
            let a = action("bm-servers", title)
            #expect(a.adminOnly && a.destructive)
            let w = try await wire(try #require(a.endpoint(for: r)))
            #expect(w.method == "POST" && w.path == "\(p)/api/v1/baremetal/servers/s-1/power" && w.body == .object(["action": .string(act)]))
            #expect((a.phrase(for: r) != nil) == (title != "Power on"))
        }
        #expect(action("bm-servers", "Power cycle").phrase(for: r) == "node 7")
        let dry = try await wire(try #require(action("bm-servers", "Dry-run power cycle").endpoint(for: r)))
        #expect(dry.body == .object(["action": .string("cycle"), "dry_run": .bool(true)]))
        let rel = try #require(action("bm-servers", "Release").endpoint(for: r))
        #expect(rel.resolvedPath == "\(p)/api/v1/baremetal/servers/s-1/release")
        #expect(!action("bm-servers", "Release").adminOnly)   // operator on the server
    }

    @Test func bareMetalProvisionWipesSoNeedsTypedProfileName() async throws {
        let a = action("bm-profiles", "Provision 1 machine")
        let r = row(["name": .string("k8s-worker")])
        #expect(a.adminOnly && a.phrase(for: r) == "k8s-worker" && a.confirm?.contains("WIPING") == true)
        let w = try await wire(try #require(a.endpoint(for: r)))
        #expect(w.method == "POST" && w.path == "\(p)/api/v1/baremetal/provision")
        #expect(w.body == .object(["profile": .string("k8s-worker"), "count": .number(1)]))
        let reg = try #require(FeatureRegistry.spec("bm-servers")?.create)
        #expect(reg.adminOnly)
        #expect(try reg.body(from: ["hostname": "n1", "bmc_address": "10.0.0.5"]) == .object(["hostname": .string("n1"), "bmc_address": .string("10.0.0.5")]))
    }

    @Test func enforcementNeedsAnExplicitBoundedLease() async throws {
        let form = try #require(FeatureRegistry.spec("bpf-mode")?.create)
        #expect(form.adminOnly && form.method == "PUT" && form.endpoint.viaController == false)
        #expect(throws: ResourceCreate.Invalid.self) { try form.body(from: [:]) }                       // no lease, no request
        #expect(throws: ResourceCreate.Invalid.self) { try form.body(from: ["lease_secs": "59"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try form.body(from: ["lease_secs": "901"]) }
        let body = try form.body(from: ["lease_secs": "300"])
        #expect(body == .object(["mode": .string("enforce"), "lease_secs": .number(300)]))
        var e = form.endpoint; e.body = body
        let w = try await wire(e)
        #expect(w.method == "PUT" && w.path == "/api/v1/bpf/mode" && w.body == body)
        let end = action("bpf-mode", "End enforcement (observe)")
        #expect(end.rowless && end.adminOnly)
        let o = try await wire(try #require(end.endpoint(for: row([:]))))
        #expect(o.method == "PUT" && o.path == "/api/v1/bpf/mode" && o.body == .object(["mode": .string("observe")]))
    }

    @Test func nodeIsolationDefaultsToDryRunAndArmingNeedsPhraseAndSafeAllowlist() async throws {
        let spec = try #require(FeatureRegistry.spec("bpf-nodeiso"))
        let dry = try #require(spec.create), arm = try #require(spec.moreCreates.first)
        #expect(spec.create == spec.allCreates.first)                       // the first (default) form is the dry run
        #expect(dry.title.hasPrefix("Dry run") && arm.title.hasPrefix("Arm"))
        let vals = ["iface": "eth0", "lease_secs": "60", "allow_tcp": dry.fields.first { $0.id == "allow_tcp" }!.initial]
        let d = try dry.body(from: vals)
        #expect(d["dry_run"] == .bool(true) && d["enabled"] == .bool(true) && d["lease_secs"] == .number(60))
        #expect(d["allow_tcp"]?.array?.count == 6)                         // defaults keep 22, 5092, 5093, ...
        // A lease is mandatory and bounded 10...900.
        #expect(throws: ResourceCreate.Invalid.self) { try dry.body(from: ["iface": "eth0"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try dry.body(from: ["iface": "eth0", "lease_secs": "9"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try dry.body(from: ["iface": "eth0", "lease_secs": "901"]) }
        // Arming: refused without the typed phrase, whatever else is right.
        #expect(arm.typedPhrase == "ISOLATE NODE" && arm.adminOnly)
        let armVals = ["iface": "eth0", "lease_secs": "30", "allow_tcp": "22,5092"]
        #expect(throws: ResourceCreate.Invalid.self) { try arm.body(from: armVals) }
        #expect(throws: ResourceCreate.Invalid.self) { try arm.body(from: armVals.merging([ResourceCreate.phraseKey: "isolate node"]) { $1 }) }
        let a = try arm.body(from: armVals.merging([ResourceCreate.phraseKey: "ISOLATE NODE"]) { $1 })
        #expect(a["dry_run"] == .bool(false) && a["enabled"] == .bool(true) && a["lease_secs"] == .number(30))
        // Lockout guard: no SSH and no exempt CIDR is refused; an exempt CIDR makes it acceptable.
        let noSSH = armVals.merging(["allow_tcp": "5092", ResourceCreate.phraseKey: "ISOLATE NODE"]) { $1 }
        #expect(throws: ResourceCreate.Invalid.self) { try arm.body(from: noSSH) }
        let withExempt = try arm.body(from: noSSH.merging(["exempt": "10.0.0.0/24, 192.168.1.5/32"]) { $1 })
        #expect(withExempt["exempt"] == .array([.string("10.0.0.0/24"), .string("192.168.1.5/32")]))
        #expect(throws: ResourceCreate.Invalid.self) { try dry.body(from: vals.merging(["allow_tcp": "22,99999"]) { $1 }) }
        // The request itself.
        var e = dry.endpoint; e.body = d
        let w = try await wire(e)
        #expect(w.method == "PUT" && w.path == "/api/v1/bpf/node-iso" && w.body == d)
        let off = action("bpf-nodeiso", "Disable isolation")
        #expect(off.rowless && off.adminOnly)
        let o = try await wire(try #require(off.endpoint(for: row([:]))))
        #expect(o.method == "PUT" && o.body == .object(["enabled": .bool(false)]))
    }

    @Test func policiesRequireAdminAndOfferNoDefaultDeny() throws {
        let spec = try #require(FeatureRegistry.spec("bpf-policies"))
        let add = try #require(spec.create)
        let kind = try #require(add.fields.first { $0.id == "kind" })
        #expect(!kind.options.contains("tc_allow"))
        #expect(throws: ResourceCreate.Invalid.self) { try add.body(from: ["id": "p1", "kind": "tc_allow", "match": "x"]) }
        let b = try add.body(from: ["id": "p1", "kind": "deny_port", "match": "23"])
        #expect(b["enabled"] == .bool(true) && b["kind"] == .string("deny_port"))
        let rm = try #require(spec.actions.first)
        #expect(rm.adminOnly && rm.endpoint(for: row(["id": .string("a b")]))?.resolvedPath == "/api/v1/bpf/policies/a%20b")
    }

    @Test func everyWriteOnTheNewBpfAndBareMetalPagesIsAdminOnly() {
        for id in ["bpf-mode", "bpf-policies", "bpf-nodeiso", "bm-profiles", "bm-servers"] {
            let s = FeatureRegistry.spec(id)!
            for a in s.actions where !(id == "bm-servers" && a.title == "Release") { #expect(a.adminOnly, "\(id) \(a.title)") }
            for c in s.allCreates { #expect(c.adminOnly, "\(id) \(c.title)") }
        }
    }
}
