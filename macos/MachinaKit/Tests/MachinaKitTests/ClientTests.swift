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
