import Testing
import Foundation
@testable import MachinaKit

/// Own URLProtocol: the shared MockProtocol handler is global state and made other suites flaky when run in parallel.
private final class FleetMock: URLProtocol, @unchecked Sendable {
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

private final class Captured: @unchecked Sendable {
    private let lock = NSLock(); private var items: [(String, String, Data?)] = []
    func add(_ m: String, _ p: String, _ b: Data?) { lock.withLock { items.append((m, p, b)) } }
    var all: [(String, String, Data?)] { lock.withLock { items } }
}

@Suite(.serialized) struct FleetEbpfTests {
    private let hostId = "8f1c2d3e-4a5b-4c6d-8e7f-0123456789ab"
    private var host: String { "\(hostId)|node-b" }
    private func spec(_ id: String) -> ResourceSpec { FleetEbpf.specs.first { $0.id == id }! }
    private func phrase(_ p: String) -> [String: String] { [ResourceCreate.phraseKey: p] }

    @Test func registeredAndAdminOnlyOneHost() {
        #expect(FleetEbpf.specs.count == 4)
        for id in FleetEbpf.specs.map(\.id) { #expect(FeatureRegistry.spec(id) != nil) }
        for s in FleetEbpf.specs {
            #expect(s.endpoint.path == "/api/v1/zeus-security/native-dataplane" && s.endpoint.viaController)
            for c in s.allCreates {
                #expect(c.adminOnly, "\(s.id) \(c.title)")
                #expect(c.controller && c.method == "POST" && c.isFleet)
                #expect(c.path == "/api/v1/zeus-security/hosts/{host}/bpf")
                #expect(c.fields.first?.id == "host" && c.fields.first?.required == true && c.fields.first?.choices != nil)
                #expect(c.warning?.contains("ONE FLEET HOST") == true)
            }
            #expect(s.actions.isEmpty)    // no row action can fan out
        }
    }

    @Test func refusesAllHostsAndMalformedHosts() {
        let c = spec("fleet-bpf-guard").allCreates[0]
        for bad in ["", "all", "*", "all|all", "\(hostId)|all", "\(hostId)|a,b", "not-a-uuid|node", "\(hostId)", "|node"] {
            #expect(throws: ResourceCreate.Invalid.self, "\(bad)") { try c.body(from: ["host": bad]) }
            #expect(throws: ResourceCreate.Invalid.self) { try c.endpoint(values: ["host": bad]) }
        }
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: [:]) }
    }

    @Test func guardAuditBodyAndPath() throws {
        let c = spec("fleet-bpf-guard").allCreates[0]
        let b = try c.body(from: ["host": host]).object!
        #expect(b["op"] == .string("guard_configure"))
        let cfg = b["config"]!.object!
        #expect(cfg["enabled"] == .bool(true)); #expect(cfg["mode"] == .string("audit")); #expect(cfg["lease_secs"] == .null)
        #expect(cfg["host"] == nil)
        let e = try c.endpoint(values: ["host": host])
        #expect(e.path == "/api/v1/zeus-security/hosts/\(hostId)/bpf"); #expect(e.method == "POST"); #expect(e.viaController)
        #expect(e.resolvedPath == "/api/v1/platform/controller/api/v1/zeus-security/hosts/\(hostId)/bpf")
    }

    @Test func guardEnforceKeepsLeasePhraseAndRange() throws {
        let c = spec("fleet-bpf-guard").allCreates[1]
        #expect(c.typedPhrase == "ENFORCE GUARD")
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["host": host, "lease_secs": "120"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["host": host].merging(phrase("ENFORCE GUARD")) { $1 }) }
        for bad in ["0", "3601", "x"] {
            #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["host": host, "lease_secs": bad].merging(phrase("ENFORCE GUARD")) { $1 }) }
        }
        let b = try c.body(from: ["host": host, "lease_secs": "120"].merging(phrase("ENFORCE GUARD")) { $1 }).object!
        #expect(b["config"]?["mode"] == .string("enforce")); #expect(b["config"]?["lease_secs"] == .number(120))
    }

    @Test func shieldRulesAreInherited() throws {
        let audit = spec("fleet-bpf-shield").allCreates[0], enforce = spec("fleet-bpf-shield").allCreates[1]
        let base = ["host": host, "iface": "veth-test0", "protected": "10.0.0.5"]
        let ab = try audit.body(from: base).object!
        #expect(ab["op"] == .string("shield_configure")); #expect(ab["config"]?["mode"] == .string("audit")); #expect(ab["config"]?["protect_all"] == .bool(false))
        #expect(throws: ResourceCreate.Invalid.self) { try audit.body(from: ["host": host, "iface": "veth-test0"]) }
        let full = base.merging(["allow": "192.0.2.10/32"]) { $1 }
        #expect(throws: ResourceCreate.Invalid.self) { try enforce.body(from: full) }                       // no phrase
        #expect(throws: ResourceCreate.Invalid.self) { try enforce.body(from: base.merging(phrase("ENFORCE SHIELD")) { $1 }) }   // no allow list
        #expect(throws: ResourceCreate.Invalid.self) { try enforce.body(from: full.merging(["allow": "nope"]) { $1 }.merging(phrase("ENFORCE SHIELD")) { $1 }) }
        let eb = try enforce.body(from: full.merging(phrase("ENFORCE SHIELD")) { $1 }).object!
        #expect(eb["config"]?["mode"] == .string("enforce")); #expect(eb["config"]?["allow"] == .array([.string("192.0.2.10/32")]))
    }

    @Test func shieldOffNeedsInterface() throws {
        let c = spec("fleet-bpf-shield").allCreates[2]
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["host": host]) }
        let b = try c.body(from: ["host": host, "iface": "eth9"]).object!
        #expect(b["config"]?["mode"] == .string("off")); #expect(b["config"]?["iface"] == .string("eth9"))
    }

    @Test func schedulerNeedsVmsAndLease() throws {
        let start = spec("fleet-bpf-scx").allCreates[0], stop = spec("fleet-bpf-scx").allCreates[1]
        #expect(throws: ResourceCreate.Invalid.self) { try start.body(from: ["host": host, "vms": "vm1"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try start.body(from: ["host": host, "lease_secs": "60"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try start.body(from: ["host": host, "vms": "vm1", "lease_secs": "9999"]) }
        let b = try start.body(from: ["host": host, "vms": "vm1 vm2", "lease_secs": "60"]).object!
        #expect(b["op"] == .string("scx_configure")); #expect(b["config"]?["enabled"] == .bool(true)); #expect(b["config"]?["lease_secs"] == .number(60))
        #expect(try stop.body(from: ["host": host]).object?["config"]?["enabled"] == .bool(false))
    }

    @Test func vmEdgeWrapsStateAndClearNeedsPhrase() throws {
        let set = spec("fleet-bpf-vmedge").allCreates[0], clear = spec("fleet-bpf-vmedge").allCreates[1]
        #expect(set.typedPhrase == "REPLACE VM EDGE")
        #expect(throws: ResourceCreate.Invalid.self) { try set.body(from: ["host": host, "name": "vm1"]) }
        let b = try set.body(from: ["host": host, "name": "vm1", "addresses": "10.0.0.5", "egress_mbps": "50"].merging(phrase("REPLACE VM EDGE")) { $1 }).object!
        #expect(b["op"] == .string("vm_edge_sync"))
        let vm = b["state"]?["vms"]?.array?.first
        #expect(vm?["name"] == .string("vm1")); #expect(vm?["isolate_ingress"] == .bool(false)); #expect(b["state"]?["policy"] == .array([]))
        #expect(throws: ResourceCreate.Invalid.self) { try clear.body(from: ["host": host]) }
        let cb = try clear.body(from: ["host": host].merging(phrase("CLEAR VM EDGE")) { $1 }).object!
        #expect(cb["state"] == .object(["vms": .array([]), "policy": .array([])]))
    }

    @Test func confirmationNamesHostAndListsChanges() throws {
        let c = spec("fleet-bpf-guard").allCreates[1]
        let values = ["host": host, "lease_secs": "120", "allow_exec": "/usr/bin/foo"].merging(phrase("ENFORCE GUARD")) { $1 }
        let text = c.confirmation(values: values, body: try c.body(from: values))
        #expect(text.contains("node-b")); #expect(text.contains(hostId)); #expect(text.contains("No other host"))
        #expect(text.contains("mode = enforce")); #expect(text.contains("lease = 120") || text.contains("lease_secs = 120"))
        #expect(text.contains("allow_exec = /usr/bin/foo"))
        let clear = spec("fleet-bpf-vmedge").allCreates[1]
        let cv = ["host": host].merging(phrase("CLEAR VM EDGE")) { $1 }
        #expect(clear.confirmation(values: cv, body: try clear.body(from: cv)).contains("Clear the whole VM edge state"))
    }

    @Test func localFormsAreUnchanged() {
        #expect(!(FeatureRegistry.spec("bpf-guard")!.create!.isFleet))
        let local = FeatureRegistry.spec("bpf-guard")!.create!
        #expect(local.fields.first?.id != "host")
        #expect((try? local.endpoint(values: [:]))?.path == "/api/v1/bpf/guard")
    }

    @Test func hostPickerListsHostsAndSendsOneRequestToOneHost() async throws {
        let seen = Captured()
        FleetMock.handler = { req in
            seen.add(req.httpMethod ?? "", req.url!.path, req.httpBody ?? req.httpBodyStream.flatMap { s in
                s.open(); defer { s.close() }
                var d = Data(); var buf = [UInt8](repeating: 0, count: 4096)
                while s.hasBytesAvailable { let n = s.read(&buf, maxLength: buf.count); if n <= 0 { break }; d.append(contentsOf: buf[0..<n]) }
                return d })
            return (200, "{}")
        }
        let cfg = URLSessionConfiguration.ephemeral; cfg.protocolClasses = [FleetMock.self]
        let client = MachinaClient(server: ServerProfile(name: "t", url: URL(string: "https://h:5092")!), session: URLSession(configuration: cfg))
        let c = spec("fleet-bpf-guard").allCreates[0]
        let json = JSONValue.array([.object(["id": .string(hostId), "hostname": .string("node-b"), "state": .string("online")]),
                                    .object(["id": .string("11111111-2222-3333-4444-555555555555"), "hostname": .string("node-c"), "state": .string("online")])])
        let opts = c.fields[0].choices!.options(from: json)
        #expect(opts.count == 2); #expect(opts[0].value == host); #expect(!opts.contains { $0.value.lowercased() == "all" })
        var e = try c.endpoint(values: ["host": opts[0].value]); e.body = try c.body(from: ["host": opts[0].value])
        _ = try await client.call(e)
        let all = seen.all
        #expect(all.count == 1)
        #expect(all[0].0 == "POST"); #expect(all[0].1.hasSuffix("/zeus-security/hosts/\(hostId)/bpf"))
        let sent = try JSONDecoder().decode(JSONValue.self, from: all[0].2 ?? Data())
        #expect(sent["op"] == .string("guard_configure"))
    }
}
