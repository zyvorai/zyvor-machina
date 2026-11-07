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
    private let lock = NSLock(); private var items: [String] = []
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
