import Testing
import Foundation
@testable import MachinaKit

final class MockProtocol: URLProtocol, @unchecked Sendable {
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

private func makeClient() -> MachinaClient {
    let cfg = URLSessionConfiguration.ephemeral
    cfg.protocolClasses = [MockProtocol.self]
    return MachinaClient(server: ServerProfile(name: "t", url: URL(string: "https://h:5092")!),
                         session: URLSession(configuration: cfg))
}

@Suite(.serialized) struct ClientTests {
    @Test func reloginOn401() async throws {
        let calls = Counter()
        MockProtocol.handler = { req in
            let path = req.url!.path
            if path == "/api/v1/auth/login" { calls.add("login"); return (200, #"{"status":"ok","username":"u"}"#) }
            calls.add("vms")
            return calls.count("vms") == 1 ? (401, "") : (200, #"[{"name":"a","state":"running","vcpus":1,"memory_mb":512}]"#)
        }
        let c = makeClient()
        try await c.login(user: "u", password: "p")
        let vms = try await c.listVMs()
        #expect(vms.count == 1)
        #expect(calls.count("login") == 2)   // initial + one re-login after the 401
    }

    @Test func powerPathAndScope() async throws {
        let seen = Counter()
        MockProtocol.handler = { req in seen.add(req.url!.absoluteString); return (200, "{}") }
        let vm = VmInfo(name: "my vm", state: "running", vcpus: 1, memoryMb: 1, libvirtConnection: "session")
        try await makeClient().power(.reboot, vm: vm)
        #expect(seen.count("https://h:5092/api/v1/vms/my%20vm/reboot?connection=session") == 1)
    }

    @Test func slashInNameStaysOneSegment() async throws {
        let seen = Counter()
        MockProtocol.handler = { req in seen.add(req.url!.absoluteString); return (200, "{}") }
        try await makeClient().power(.start, vm: VmInfo(name: "a/b", state: "shutoff", vcpus: 1, memoryMb: 1))
        #expect(seen.count("https://h:5092/api/v1/vms/a%2Fb/start") == 1)
    }

    @Test func httpErrorSurfaces() async {
        MockProtocol.handler = { _ in (500, "boom") }
        await #expect(throws: MachinaError.self) { try await makeClient().listVMs() }
    }

    @Test func watchURLUsesWss() async throws {
        let u = try await makeClient().watchURL(token: "abc")
        #expect(u.absoluteString == "wss://h:5092/ws/v1/watch?token=abc")
    }
}

final class Counter: @unchecked Sendable {
    fileprivate let lock = NSLock(); fileprivate var items: [String] = []
    func add(_ s: String) { lock.withLock { items.append(s) } }
    func count(_ s: String) -> Int { lock.withLock { items.filter { $0 == s }.count } }
}

// Same suite as ClientTests: the mock handler is shared static state, so tests must not run in parallel.
extension ClientTests {
    @Test func decodesHostsTasksAlertsMetrics() async throws {
        MockProtocol.handler = { req in
            switch req.url!.path {
            case "/api/v1/platform/controller/api/v1/hosts":
                return (200, #"[{"id":"h1","hostname":"n1","address":"10.0.0.1","state":"online","vm_count":6,"cpu_percent":29.3,"memory_used_mib":19588,"memory_total_mib":31423,"maintenance_mode":false,"agent_grpc_addr":"x","extra":1}]"#)
            case "/api/v1/platform/controller/api/v1/tasks":
                return (200, #"[{"id":"t1","operation":"host.inventory","status":"failed","progress":0,"message":"transport error","created_at":"2026-11-07T13:00:11Z"}]"#)
            case "/api/v1/fleet/alerts":
                return (200, #"{"enabled":false,"total_unacknowledged":2,"peers":[{"peer":"local","alerts":[{"id":"a1","rule_name":"High CPU","message":"m","severity":"critical","timestamp":"2026-10-23 19:58:35","acknowledged":false}]}]}"#)
            default:
                return (200, #"{"enabled":false,"local":{"host_cpu_percent":28.5,"host_memory_percent":62.2,"host_disk_percent":74.3,"load_1":3.59,"vm_count":4,"vms_running":4,"capacity":{"score":45.0,"label":"high"}},"peers":[]}"#)
            }
        }
        let c = makeClient()
        let h = try await c.hosts()
        #expect(h[0].isOnline && h[0].vmCount == 6)
        #expect(Int(h[0].memoryPercent ?? 0) == 62)
        let t = try await c.tasks()
        #expect(t[0].isFailed && !t[0].isActive)
        let a = try await c.alerts()
        #expect(a.totalUnacknowledged == 2 && a.alerts[0].peer == "local" && a.alerts[0].ruleName == "High CPU")
        let m = try await c.localMetrics()
        #expect(m.vmsRunning == 4 && m.capacity?.label == "high")
    }

    @Test func taskActionPaths() async throws {
        let seen = Counter()
        MockProtocol.handler = { req in seen.add("\(req.httpMethod ?? "") \(req.url!.path)"); return (200, "{}") }
        let c = makeClient()
        try await c.cancelTask("abc")
        try await c.retryTask("abc")
        #expect(seen.count("POST /api/v1/platform/controller/api/v1/tasks/abc/cancel") == 1)
        #expect(seen.count("POST /api/v1/platform/controller/api/v1/tasks/abc/retry") == 1)
    }
}

extension ClientTests {
    @Test func jsonValueTextAndRows() throws {
        let json = try JSONDecoder().decode(JSONValue.self, from: Data(#"{"events":[{"id":"a","n":3,"ok":true,"x":null,"l":[1,2]},{"id":"b","n":2.5}],"k":"v"}"#.utf8))
        let spec = ResourceSpec("t", "T", group: "g", icon: "i", endpoint: .get("/x"), rowsPath: ["events"], idKey: "id")
        let rows = spec.rows(from: json)
        #expect(rows.map(\.id) == ["a", "b"])
        #expect(rows[0].values["n"]?.text == "3" && rows[1].values["n"]?.text == "2.5")
        #expect(rows[0].values["ok"]?.text == "yes" && rows[0].values["x"]?.text == "–" && rows[0].values["l"]?.text == "1, 2")
        // Auto columns: scalar keys only, sorted, capped at 6.
        #expect(spec.effectiveColumns(for: rows).map(\.key) == ["id", "n", "ok", "x"])
    }

    @Test func keyValueLayoutAndFormats() throws {
        let json = try JSONDecoder().decode(JSONValue.self, from: Data(#"{"b":2,"a":"x"}"#.utf8))
        let spec = ResourceSpec("t", "T", group: "g", icon: "i", endpoint: .get("/x"), layout: .keyValue)
        let rows = spec.rows(from: json)
        #expect(rows.map(\.id) == ["a", "b"])
        #expect(spec.effectiveColumns(for: rows).map(\.title) == ["Key", "Value"])
        let gb = ResourceColumn("c", "v", .gigabytes)
        #expect(gb.render(ResourceRow(id: "1", values: .object(["v": .number(914.78)]))) == "914.8 GB")
    }

    @Test func registryIsConsistent() {
        let ids = FeatureRegistry.all.map(\.id)
        #expect(Set(ids).count == ids.count)
        #expect(FeatureRegistry.groups.flatMap(\.specs).count == ids.count)
        #expect(FeatureRegistry.spec("ha")?.endpoint.resolvedPath == "/api/v1/platform/controller/api/v1/ha/status")
    }

    @Test func callRejectsHtmlAndDecodesJson() async throws {
        MockProtocol.handler = { req in
            req.url!.path == "/api/v1/events" ? (200, "<!DOCTYPE html><html></html>") : (200, #"{"role":"admin","authenticated":true}"#)
        }
        let c = makeClient()
        await #expect(throws: MachinaError.self) { _ = try await c.call(.get("/api/v1/events")) }
        let v = try await c.call(.get("/api/v1/auth/session"))
        #expect(v["role"]?.text == "admin")
        #expect(try await c.sessionInfo().role == "admin")
    }
}

extension ClientTests {
    @Test func actionEndpointsAndBodies() async throws {
        let seen = Counter()
        MockProtocol.handler = { req in
            var s = "\(req.httpMethod ?? "") \(req.url!.absoluteString)"
            if let b = req.httpBody ?? req.httpBodyStream.map({ st in st.open(); defer { st.close() }; var d = Data(); var buf = [UInt8](repeating: 0, count: 1024)
                while st.hasBytesAvailable { let n = st.read(&buf, maxLength: 1024); if n <= 0 { break }; d.append(buf, count: n) }; return d }) {
                s += " " + String(decoding: b, as: UTF8.self)
            }
            seen.add(s); return (200, "{}")
        }
        let c = makeClient()
        let vm = VmInfo(name: "a b", state: "shutoff", vcpus: 1, memoryMb: 512, libvirtConnection: "session")
        try await c.createSnapshot(vm, name: "s1", description: "d")
        try await c.revertSnapshot(vm, "s/1")
        try await c.deleteSnapshot(vm, "s1")
        try await c.setAutostart(vm, true)
        try await c.resizeDisk(vm, target: "vdb", sizeGb: 5)
        try await c.detachDisk(vm, target: "vdb")
        try await c.deleteVM(vm)
        try await c.restoreBackup("b1")
        let all = seen.all()
        #expect(all.contains { $0.hasPrefix("POST https://h:5092/api/v1/vms/a%20b/snapshots?connection=session") && $0.contains(#""name":"s1""#) })
        #expect(all.contains("POST https://h:5092/api/v1/vms/a%20b/snapshots/s%2F1/revert?connection=session"))
        #expect(all.contains("DELETE https://h:5092/api/v1/vms/a%20b/snapshots/s1?connection=session"))
        #expect(all.contains("POST https://h:5092/api/v1/vms/a%20b/autostart/true?connection=session"))
        #expect(all.contains { $0.hasPrefix("POST https://h:5092/api/v1/vms/a%20b/disk/resize/vdb") && $0.contains(#""size_gb":5"#) })
        #expect(all.contains("POST https://h:5092/api/v1/vms/a%20b/disk/detach/vdb?connection=session"))
        #expect(all.contains("DELETE https://h:5092/api/v1/vms/a%20b?connection=session"))
        #expect(all.contains { $0.hasPrefix("POST https://h:5092/api/v1/backups/restore") && $0.contains(#""backup_id":"b1""#) })
    }

    @Test func resourceActionResolvesRowValues() {
        let row = ResourceRow(id: "0", values: .object(["vm_name": .string("my vm"), "name": .string("snap/1"), "id": .string("b1")]))
        let revert = ResourceAction("Revert", "/api/v1/vms/{vm_name}/snapshots/{name}/revert", destructive: true)
        #expect(revert.endpoint(for: row)?.path == "/api/v1/vms/my%20vm/snapshots/snap%2F1/revert")
        #expect(ResourceAction("X", "/a/{missing}").endpoint(for: row) == nil)   // never send a half-built path
        let restore = ResourceAction("Restore", "/api/v1/backups/restore", bodyFields: ["backup_id": "id"])
        #expect(restore.endpoint(for: row)?.body == .object(["backup_id": .string("b1")]))
        #expect(ResourceAction("C", "/c/{id}", controller: true).endpoint(for: row)?.resolvedPath == "/api/v1/platform/controller/c/b1")
    }

    @Test func detailsAndCreateRequestShape() async throws {
        MockProtocol.handler = { _ in (200, #"{"name":"v","state":"running","vcpus":2,"memory_mb":1024,"disks":[{"device":"disk","target":"vda","source":"/x.qcow2"},{"device":"cdrom","target":"sda"}],"interfaces":[{"mac_address":"aa:bb","source":"default","model":"virtio"}]}"#) }
        let d = try await makeClient().details(VmInfo(name: "v", state: "running", vcpus: 2, memoryMb: 1024))
        #expect(d.disks?.count == 2 && d.disks?[1].isCdrom == true && d.interfaces?.first?.macAddress == "aa:bb")
        let r = CreateVMRequest(name: "n", vcpus: 1, memoryMb: 128, diskGb: 1, network: "default").json
        #expect(r["disk_gb"]?.double == 1 && r["network"]?.text == "default" && r["iso"] == nil)
    }
}

extension Counter { func all() -> [String] { lock.withLock { items } } }
