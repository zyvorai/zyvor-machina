import Testing
import Foundation
@testable import MachinaKit

private final class Seen: @unchecked Sendable {
    private let lock = NSLock(); private var items: [(String, String, Data?)] = []
    func add(_ m: String, _ p: String, _ b: Data?) { lock.withLock { items.append((m, p, b)) } }
    var all: [(String, String, Data?)] { lock.withLock { items } }
}

/// Own mock (not `EbpfMock`): its handler is static state shared with the other suites, which run in parallel.
private final class EbpfMock: URLProtocol, @unchecked Sendable {
    nonisolated(unsafe) static var handler: (@Sendable (URLRequest) -> (Int, String))?
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        let (code, body) = Self.handler?(request) ?? (500, "")
        let resp = HTTPURLResponse(url: request.url!, statusCode: code, httpVersion: nil, headerFields: nil)!
        client?.urlProtocol(self, didReceive: resp, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(body.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

@Suite(.serialized) struct EbpfFormsTests {
    private func spec(_ id: String) -> ResourceSpec { FeatureRegistry.spec(id)! }
    private let empty = ResourceRow(id: "", values: .object([:]))
    private func guardEnforce() -> ResourceCreate { spec("bpf-guard").moreCreates[0] }
    private func shieldEnforce() -> ResourceCreate { spec("bpf-shield").moreCreates[0] }

    // MARK: role gating and registry

    @Test func everyWriteIsAdminOnlyAndDaemonSide() {
        let specs = EbpfForms.writeSpecs
        #expect(specs.count == 4)
        for s in specs {
            #expect(!s.allCreates.isEmpty || !s.actions.isEmpty)
            for c in s.allCreates { #expect(c.adminOnly, "\(s.id) \(c.title)"); #expect(c.method == "PUT"); #expect(!c.controller); #expect(c.warning != nil) }
            for a in s.actions { #expect(a.adminOnly, "\(s.id) \(a.title)"); #expect(a.method == "PUT"); #expect(a.confirm != nil) }
        }
        #expect(Set(FeatureRegistry.all.map(\.id)).count == FeatureRegistry.all.count)
        #expect(FeatureRegistry.all.filter { $0.id == "bpf-guard" }.count == 1)
    }

    // MARK: guard

    @Test func guardAuditSendsNoLeaseAndNeverEnforces() throws {
        let c = spec("bpf-guard").create!
        let b = try c.body(from: ["exec": "yes", "wx": "yes", "devices": "no", "allow_exec": "/usr/bin/x, /bin/y"]).object!
        #expect(b["mode"] == .string("audit")); #expect(b["enabled"] == .bool(true)); #expect(b["lease_secs"] == .null)
        #expect(b["devices"] == .bool(false)); #expect(b["allow_exec"] == .array([.string("/usr/bin/x"), .string("/bin/y")]))
        #expect(c.path == "/api/v1/bpf/guard")
    }

    @Test func guardEnforceNeedsExplicitLeaseAndPhrase() throws {
        let c = guardEnforce()
        #expect(c.typedPhrase == "ENFORCE GUARD")
        let ok = ["lease_secs": "120", ResourceCreate.phraseKey: "ENFORCE GUARD"]
        let b = try c.body(from: ok).object!
        #expect(b["mode"] == .string("enforce")); #expect(b["lease_secs"] == .number(120))
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["lease_secs": "120"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: [ResourceCreate.phraseKey: "ENFORCE GUARD"]) }
        for bad in ["0", "3601", "-5", "x", ""] {
            #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["lease_secs": bad, ResourceCreate.phraseKey: "ENFORCE GUARD"]) }
        }
        #expect(try c.body(from: ["lease_secs": "1", ResourceCreate.phraseKey: "ENFORCE GUARD"]).object?["lease_secs"] == .number(1))
        #expect(try c.body(from: ["lease_secs": "3600", ResourceCreate.phraseKey: "ENFORCE GUARD"]).object?["lease_secs"] == .number(3600))
    }

    @Test func guardOffActionBody() {
        let a = spec("bpf-guard").actions[0]
        let b = a.endpoint(for: empty)!.body!.object!
        #expect(b["enabled"] == .bool(false)); #expect(b["mode"] == .string("audit")); #expect(a.global)
    }

    // MARK: shield

    private let shieldBase = ["iface": "veth-test0", "protected": "10.0.0.5", "allow": "192.0.2.10/32"]

    @Test func shieldAuditBody() throws {
        let c = spec("bpf-shield").create!
        let b = try c.body(from: shieldBase.merging(["syn_pps": "500"]) { $1 }).object!
        #expect(b["mode"] == .string("audit")); #expect(b["iface"] == .string("veth-test0")); #expect(b["syn_pps"] == .number(500))
        #expect(b["protected"] == .array([.string("10.0.0.5")])); #expect(b["protect_all"] == .bool(false))
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["iface": "veth-test0"]) }  // protects nothing
        #expect(try c.body(from: ["iface": "veth-test0", "protect_all": "yes"]).object?["protect_all"] == .bool(true))
    }

    @Test func shieldEnforceRefusesWithoutAllowListPhraseOrTarget() throws {
        let c = shieldEnforce()
        let phrase = [ResourceCreate.phraseKey: "ENFORCE SHIELD"]
        let good = shieldBase.merging(phrase) { $1 }
        #expect(try c.body(from: good).object?["mode"] == .string("enforce"))
        var noAllow = good; noAllow["allow"] = ""
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: noAllow) }
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: shieldBase) }
        var noTarget = good; noTarget["protected"] = ""
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: noTarget) }
        var wrong = good; wrong[ResourceCreate.phraseKey] = "enforce shield"
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: wrong) }
    }

    @Test func shieldRejectsBadAddressesAndRanges() {
        let c = shieldEnforce()
        let phrase = [ResourceCreate.phraseKey: "ENFORCE SHIELD"]
        for bad in ["nope", "10.0.0.0/33", "1.2.3", "::1/129"] {
            var v = shieldBase.merging(phrase) { $1 }; v["allow"] = bad
            #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: v) }
        }
        var d = shieldBase.merging(phrase) { $1 }; d["deny"] = "198.51.100.0/24, fd00::/8"
        #expect((try? c.body(from: d)) != nil)
        var burst = shieldBase.merging(phrase) { $1 }; burst["burst_secs"] = "61"
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: burst) }
    }

    @Test func shieldOffActionAsksForInterface() throws {
        let a = spec("bpf-shield").actions[0]
        #expect(throws: ResourceCreate.Invalid.self) { try a.endpoint(for: empty, input: [:]) }
        let e = try a.endpoint(for: empty, input: ["iface": "eth9"])!
        #expect(e.body?.object?["mode"] == .string("off")); #expect(e.body?.object?["iface"] == .string("eth9"))
    }

    // MARK: scheduler

    @Test func scxStartNeedsVmsAndExplicitLease() throws {
        let c = spec("bpf-scx").create!
        let b = try c.body(from: ["vms": "vm-a, vm-b", "lease_secs": "60", "latency_target_us": "500"]).object!
        #expect(b["enabled"] == .bool(true)); #expect(b["lease_secs"] == .number(60)); #expect(b["latency_target_us"] == .number(500))
        #expect(b["vms"] == .array([.string("vm-a"), .string("vm-b")]))
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["vms": "vm-a"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["lease_secs": "60"]) }
        for bad in ["0", "3601", "-1"] {
            #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["vms": "vm-a", "lease_secs": bad]) }
        }
        let stop = spec("bpf-scx").actions[0].endpoint(for: empty)!
        #expect(stop.body?.object == ["enabled": .bool(false)])
    }

    // MARK: VM edge

    @Test func vmEdgeBodyIsNestedAndDefaultsToNoIsolation() throws {
        let c = spec("bpf-vmedge").create!
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["name": "vm-a"]) }  // phrase missing
        let p = ResourceCreate.phraseKey
        let b = try c.body(from: ["name": "vm-a", "addresses": "10.1.0.4, fd00::4", "egress_mbps": "100", p: "REPLACE VM EDGE"]).object!
        let vm = b["vms"]!.array![0].object!
        #expect(vm["name"] == .string("vm-a")); #expect(vm["isolate_ingress"] == .bool(false)); #expect(vm["isolate_egress"] == .bool(false))
        #expect(vm["egress_mbps"] == .number(100)); #expect(vm["ingress_mbps"] == nil)
        #expect(b["policy"] == .array([]))
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["name": "vm-a", "addresses": "bogus", p: "REPLACE VM EDGE"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["name": "", p: "REPLACE VM EDGE"]) }
    }

    @Test func vmEdgeClearBody() {
        let b = spec("bpf-vmedge").actions[0].endpoint(for: empty)!.body!.object!
        #expect(b["vms"] == .array([])); #expect(b["policy"] == .array([]))
    }

    // MARK: wire

    @Test func guardEnforceRequestOnTheWire() async throws {
        let seen = Seen()
        EbpfMock.handler = { req in
            let body = req.httpBody ?? req.httpBodyStream.map { st in
                st.open(); defer { st.close() }
                var d = Data(); var buf = [UInt8](repeating: 0, count: 4096)
                while st.hasBytesAvailable { let n = st.read(&buf, maxLength: buf.count); if n <= 0 { break }; d.append(buf, count: n) }
                return d
            }
            seen.add(req.httpMethod ?? "", req.url!.path, body)
            return (200, #"{"config":{"enabled":true}}"#)
        }
        let cfg = URLSessionConfiguration.ephemeral; cfg.protocolClasses = [EbpfMock.self]
        let client = MachinaClient(server: ServerProfile(name: "t", url: URL(string: "https://h:5092")!), session: URLSession(configuration: cfg))
        let c = guardEnforce()
        let body = try c.body(from: ["lease_secs": "90", ResourceCreate.phraseKey: "ENFORCE GUARD"])
        _ = try await client.call(Endpoint(c.method, c.path, body: body, controller: c.controller))
        let req = try #require(seen.all.last { $0.1 == "/api/v1/bpf/guard" })
        #expect(req.0 == "PUT")
        let sent = try JSONDecoder().decode(JSONValue.self, from: req.2 ?? Data())
        #expect(sent["lease_secs"] == .number(90)); #expect(sent["mode"] == .string("enforce"))
    }
}
