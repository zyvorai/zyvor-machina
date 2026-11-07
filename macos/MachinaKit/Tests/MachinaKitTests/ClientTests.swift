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

extension ClientTests {
    private func pt(_ ms: Double, cpu: Double, rx: Double, mem: Double = 100) -> MetricsPoint {
        MetricsPoint(timestampMs: ms, hostCpuPercent: 10, hostMemoryPercent: 20, hostDiskPercent: 30, load1: 1, vmsRunning: 1, vmCount: 1,
                     vmMetrics: [VmMetricsSample(name: "a", state: "running", cpuTimeNs: cpu, vcpus: 2, memoryTotalMb: 512, memoryUsedMb: mem,
                                                 memoryPct: nil, diskRdBytes: 0, diskWrBytes: 0, netRxBytes: rx, netTxBytes: 0)])
    }

    @Test func metricsRatesFromCumulativeCounters() {
        // 10 s apart; 2 vCPUs used 10 s of CPU in total => 50 %. 5000 bytes in 10 s => 500 B/s.
        let pts = [pt(0, cpu: 0, rx: 0), pt(10_000, cpu: 10e9, rx: 5000), pt(20_000, cpu: 10e9 + 2e9, rx: 6000)]
        let cpu = MetricsMath.vmCPU(pts, vm: "a")
        #expect(cpu.count == 2 && abs(cpu[0].value - 50) < 0.001 && abs(cpu[1].value - 10) < 0.001)
        let rx = MetricsMath.vmRates(pts, vm: "a", [.netRx])
        #expect(rx.map(\.value) == [500, 100] && rx[0].series == "Network in")
        #expect(MetricsMath.vmMemory(pts, vm: "a").count == 3)
        #expect(MetricsMath.hostSeries(pts).count == 9)
    }

    @Test func metricsSkipCounterResetsAndGaps() {
        let reset = [pt(0, cpu: 9e9, rx: 9000), pt(10_000, cpu: 1e9, rx: 100), pt(10_000, cpu: 2e9, rx: 200)]
        #expect(MetricsMath.vmCPU(reset, vm: "a").isEmpty)                  // went backwards, then zero time gap
        #expect(MetricsMath.vmRates(reset, vm: "a", [.netRx]).isEmpty)
        #expect(MetricsMath.vmCPU([pt(0, cpu: 0, rx: 0)], vm: "missing").isEmpty)
    }

    @Test func metricsHistoryDecodes() async throws {
        MockProtocol.handler = { _ in (200, #"{"points":[{"timestamp_ms":1794057917139,"host_cpu_percent":29.0,"load_1":5.11,"vms_running":4,"vm_count":4,"vm_metrics":[{"name":"w","cpu_time_ns":5.6e12,"vcpus":1,"memory_used_mb":30,"net_rx_bytes":29156161,"extra":1}]}]}"#) }
        let pts = try await makeClient().metricsHistory(limit: 5)
        #expect(pts.count == 1 && pts[0].vmMetrics?[0].netRxBytes == 29156161 && pts[0].load1 == 5.11)
    }
}

// MARK: Console (RFB, keys, terminal)

final class ScriptedTransport: ConsoleTransport, @unchecked Sendable {
    private let lock = NSLock()
    private var chunks: [Data]
    private(set) var sent: [Data] = []
    init(_ chunks: [[UInt8]]) { self.chunks = chunks.map { Data($0) } }
    func receive() async throws -> Data {
        let next: Data? = lock.withLock { chunks.isEmpty ? nil : chunks.removeFirst() }
        guard let next else { throw RFBError.closed }
        return next
    }
    func send(_ data: Data) async throws { lock.withLock { sent.append(data) } }
    func close() {}
    var sentBytes: [[UInt8]] { lock.withLock { sent.map { Array($0) } } }
}

private func be16(_ v: Int) -> [UInt8] { [UInt8(v >> 8), UInt8(v & 0xff)] }
private func be32(_ v: Int) -> [UInt8] { [UInt8(v >> 24 & 255), UInt8(v >> 16 & 255), UInt8(v >> 8 & 255), UInt8(v & 255)] }
private func handshake(w: Int, h: Int, version: String = "RFB 003.008\n") -> [[UInt8]] {
    var first = Array(version.utf8)
    var rest: [UInt8] = version.contains("003.008") ? [1, 1] + be32(0) : version.contains("003.007") ? [1, 1] : be32(1)
    rest += be16(w) + be16(h) + [UInt8](repeating: 0, count: 16) + be32(4) + Array("test".utf8)
    first += rest   // one chunk on purpose: the client must split a stream, not rely on message boundaries
    return [first]
}

extension ClientTests {
    @Test func rfbHandshakeSendsExpectedMessages() async throws {
        let t = ScriptedTransport(handshake(w: 4, h: 2))
        let c = RFBClient(transport: t)
        try await c.connect()
        let (w, h, name) = (await c.width, await c.height, await c.desktopName)
        #expect(w == 4 && h == 2 && name == "test")
        let sent = t.sentBytes
        #expect(sent[0] == Array("RFB 003.008\n".utf8))
        #expect(sent[1] == [1])                                   // security None
        #expect(sent[2] == [1])                                   // ClientInit shared
        #expect(sent[3].count == 20 && sent[3][0] == 0 && sent[3][4] == 32)    // SetPixelFormat, 32 bpp
        #expect(sent[4][0] == 2 && sent[4].count == 4 + 16)       // SetEncodings, 4 encodings
        #expect(sent[5] == [3, 0] + be16(0) + be16(0) + be16(4) + be16(2))     // full update request
    }

    @Test func rfbOlderVersionsAndPasswordRefusal() async throws {
        let c37 = RFBClient(transport: ScriptedTransport(handshake(w: 1, h: 1, version: "RFB 003.007\n")))
        try await c37.connect()
        let c33 = RFBClient(transport: ScriptedTransport(handshake(w: 1, h: 1, version: "RFB 003.003\n")))
        try await c33.connect()
        let pw = RFBClient(transport: ScriptedTransport([Array("RFB 003.008\n".utf8) + [1, 2]]))   // only VNC auth offered
        await #expect(throws: RFBError.self) { try await pw.connect() }
        let junk = RFBClient(transport: ScriptedTransport([Array("HTTP/1.1 400 \n".utf8)]))
        await #expect(throws: RFBError.self) { try await junk.connect() }
    }

    @Test func rfbDecodesRawCopyRectAndHextile() async throws {
        var msgs = handshake(w: 4, h: 2)
        // Raw: pixel (1,0) = B10 G20 R30; pixel (2,0) = B1 G2 R3.
        var upd: [UInt8] = [0, 0] + be16(3)
        upd += be16(1) + be16(0) + be16(2) + be16(1) + be32(0) + [10, 20, 30, 0, 1, 2, 3, 0]
        // CopyRect: copy (1,0) 1x1 to (0,1).
        upd += be16(0) + be16(1) + be16(1) + be16(1) + be32(1) + be16(1) + be16(0)
        // Hextile over (2,1) 2x1: bg+fg specified, subrects, not coloured: bg red(0,0,255), fg green, one subrect at x=1,y=0 1x1.
        upd += be16(2) + be16(1) + be16(2) + be16(1) + be32(5) + [2 | 4 | 8, 0, 0, 255, 0, 0, 255, 0, 0, 1, 0x10, 0x00]
        msgs.append(upd)
        let t = ScriptedTransport(msgs)
        let c = RFBClient(transport: t)
        try await c.connect()
        await c.run()      // consumes the update, then the scripted stream ends
        let px = await c.rawPixels()
        func at(_ x: Int, _ y: Int) -> [UInt8] { Array(px.bgra[((y * px.width + x) * 4)..<((y * px.width + x) * 4 + 4)]) }
        #expect(at(1, 0) == [10, 20, 30, 255] && at(2, 0) == [1, 2, 3, 255])
        #expect(at(0, 1) == [10, 20, 30, 255])                  // copied
        #expect(at(2, 1) == [0, 0, 255, 255])                   // hextile background (red)
        #expect(at(3, 1) == [0, 255, 0, 255])                   // hextile foreground subrect (green)
        #expect(t.sentBytes.last == [3, 1] + be16(0) + be16(0) + be16(4) + be16(2))    // asks for the next incremental update
    }

    @Test func rfbInputMessages() async throws {
        let t = ScriptedTransport(handshake(w: 100, h: 50))
        let c = RFBClient(transport: t)
        try await c.connect()
        await c.key(0xff0d, down: true)
        await c.pointer(x: 500, y: -3, buttonMask: 1)           // clamped into the framebuffer
        await c.sendClipboard("hi")
        let s = t.sentBytes.suffix(3)
        #expect(Array(s[s.startIndex]) == [4, 1, 0, 0] + be32(0xff0d))
        #expect(Array(s[s.startIndex + 1]) == [5, 1] + be16(99) + be16(0))
        #expect(Array(s[s.startIndex + 2]) == [6, 0, 0, 0] + be32(2) + [104, 105])
    }

    @Test func keyMapping() {
        #expect(KeySyms.keysym(keyCode: 36, characters: "\r") == 0xff0d)
        #expect(KeySyms.keysym(keyCode: 0, characters: "a") == 0x61)
        #expect(KeySyms.keysym(keyCode: 0, characters: "é") == 0xe9)
        #expect(KeySyms.keysym(keyCode: 0, characters: "€") == 0x0100_20ac)
        #expect(KeySyms.modifier(keyCode: 56) == 0xffe1 && KeySyms.modifier(keyCode: 55) == 0xffeb)
        #expect(TerminalKeys.bytes(keyCode: 0, characters: "c", control: true) == [3])           // Ctrl-C
        #expect(TerminalKeys.bytes(keyCode: 126, characters: nil, control: false) == Array("\u{1b}[A".utf8))
        #expect(TerminalKeys.bytes(keyCode: 36, characters: "\r", control: false) == [13])
        #expect(TerminalKeys.bytes(keyCode: 0, characters: "é", control: false) == Array("é".utf8))
    }

    @Test func terminalBufferRendersLikeAConsole() {
        var t = TerminalBuffer()
        t.feed("login: root\r\nPassword: ")
        t.feed("\u{1b}[31mred\u{1b}[0m\u{1b}]0;title\u{07} ok\r\n")
        #expect(t.text == "login: root\nPassword: red ok\n")
        t.feed("abc\u{08}\u{08}X")                                   // backspace then overwrite
        #expect(t.text.hasSuffix("\naXc"))
        t.feed("\r\u{1b}[K")                                        // CR + erase line
        #expect(t.text.hasSuffix("\n"))
        var split = TerminalBuffer(); split.feed("a\u{1b}"); split.feed("[3"); split.feed("1mb")     // sequence split across chunks
        #expect(split.text == "ab")
        var cap = TerminalBuffer(maxLines: 3); for i in 0..<10 { cap.feed("l\(i)\n") }
        #expect(cap.lines.count == 3)
    }
}

extension ClientTests {
    @Test func dottedPathsAndFlattening() throws {
        let json = try JSONDecoder().decode(JSONValue.self, from: Data(#"{"items":[{"metadata":{"name":"p","uid":"u1"},"status":{"phase":"Running"}}],"a":{"b":{"c":1,"d":{"e":2}},"f":"x"},"l":[1]}"#.utf8))
        let spec = ResourceSpec("t", "T", group: "g", icon: "i", endpoint: .get("/x"), rowsPath: ["items"], idKey: "metadata.uid",
                                columns: [.init("Name", "metadata.name"), .init("Phase", "status.phase"), .init("Missing", "status.nope")])
        let rows = spec.rows(from: json)
        #expect(rows[0].id == "u1")
        #expect(spec.columns.map { $0.render(rows[0]) } == ["p", "Running", "–"])
        let kv = ResourceSpec("k", "K", group: "g", icon: "i", endpoint: .get("/x"), layout: .keyValue).rows(from: json)
        // Objects are expanded two levels; deeper values stay as one cell.
        #expect(kv.map(\.id) == ["a.b.c", "a.b.d", "a.f", "items", "l"])
    }

    @Test func actionQueryAndStaticBody() {
        let row = ResourceRow(id: "0", values: .object(["id": .string("abc"), "name": .string("n")]))
        let rm = ResourceAction("Remove", "DELETE", "/api/v1/vessel/containers/{id}", destructive: true, query: [.init(name: "force", value: "false")])
        let e = rm.endpoint(for: row)
        #expect(e?.method == "DELETE" && e?.path == "/api/v1/vessel/containers/abc" && e?.query == [.init(name: "force", value: "false")])
        let ap = ResourceAction("Approve", "/api/v1/zeus-firewall/approvals/{id}/approve", controller: true, staticBody: .object([:]))
        #expect(ap.endpoint(for: row)?.body == .object([:]) && ap.endpoint(for: row)?.resolvedPath.hasPrefix("/api/v1/platform/controller/") == true)
    }

    @Test func newPendingApprovalsOnlyAfterFirstPoll() {
        let now = [(id: "a", status: "pending"), (id: "b", status: "approved"), (id: "c", status: "Pending")]
        #expect(Approvals.newPending(seen: nil, current: now).isEmpty)                       // first poll: stay quiet
        #expect(Approvals.newPending(seen: ["a"], current: now) == ["c"])                   // b is not pending, a already seen
    }

    @Test func registryHasPhase5Pages() {
        let ids = Set(FeatureRegistry.all.map(\.id))
        #expect(["containers", "k8s-pods", "k8s-events", "approvals", "users", "fw-targets", "drs"].allSatisfy(ids.contains))
        #expect(FeatureRegistry.spec("approvals")?.actions.map(\.title) == ["Approve", "Reject"])
    }
}

extension ClientTests {
    @Test func bytesFormatAndPhase6Registry() {
        let row = ResourceRow(id: "0", values: .object(["n": .number(1_048_576), "s": .string("x")]))
        #expect(ResourceColumn("c", "n", .bytes).render(row).contains("MB"))
        #expect(ResourceColumn("c", "s", .bytes).render(row) == "x")          // not a number: shown as is
        let groups = Dictionary(grouping: FeatureRegistry.all, by: \.group)
        #expect(groups["Security"]?.count ?? 0 >= 10 && groups["Fleet security"] != nil && groups["Bare metal"] != nil && groups["Insight"] != nil)
        // Phase 6 is read-only: none of its groups carries an action.
        #expect(["Security", "Fleet security", "Bare metal", "Insight"].allSatisfy { g in groups[g]!.allSatisfy { $0.actions.isEmpty } })
    }
}

extension ClientTests {
    @Test func createFormBuildsTypedBodyAndValidates() throws {
        let c = FeatureRegistry.spec("fc-flavors")!.create!
        let body = try c.body(from: ["name": " small ", "vcpus": "2", "memory_mib": "2048", "disk_gib": "20", "description": ""])
        #expect(body["name"]?.text == "small" && body["vcpus"]?.double == 2 && body["description"] == nil)   // trimmed, typed, empty optional left out
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["name": "", "vcpus": "2", "memory_mib": "1", "disk_gib": "1"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["name": "x", "vcpus": "two", "memory_mib": "1", "disk_gib": "1"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["name": "x", "vcpus": "-1", "memory_mib": "1", "disk_gib": "1"]) }
        #expect(c.endpoint.resolvedPath == "/api/v1/platform/controller/api/v1/flavors" && c.endpoint.method == "POST")
    }

    @Test func serverGroupsAreDerivedFromTags() async throws {
        MockProtocol.handler = { _ in (200, #"[{"name":"a","tags":["anti-affinity:db","x"]},{"name":"b","tags":["anti-affinity:db"]},{"name":"c","tags":["anti-affinity:web"]},{"name":"d","tags":[]}]"#) }
        let spec = FeatureRegistry.spec("fc-servergroups")!
        let rows = spec.rows(from: try await makeClient().load(spec))
        #expect(rows.map(\.id) == ["0", "1"])
        #expect(spec.columns.map { $0.render(rows[0]) } == ["db", "2", "a, b"] && spec.columns.map { $0.render(rows[1]) } == ["web", "1", "c"])
    }

    @Test func fanOutMergesChildrenAndToleratesOneFailure() async throws {
        MockProtocol.handler = { req in
            switch req.url!.path {
            case "/api/v1/platform/controller/api/v1/vms": return (200, #"[{"id":"1","name":"a"},{"id":"2","name":"b"},{"id":"3","name":"c"}]"#)
            case "/api/v1/platform/controller/api/v1/vms/1/port-forwards": return (200, #"[{"port":80},{"port":443}]"#)
            case "/api/v1/platform/controller/api/v1/vms/2/port-forwards": return (404, "no")
            default: return (200, #"{"items":[{"port":22}]}"#)
            }
        }
        let spec = FeatureRegistry.spec("fc-portforwards")!
        let rows = spec.rows(from: try await makeClient().load(spec))
        #expect(rows.count == 3)
        #expect(rows.map { $0.values["_parent"]?.text ?? "" } == ["a", "a", "c"])      // b failed and is skipped, order follows the parents
        #expect(spec.effectiveColumns(for: rows).first?.title == "Parent")
        MockProtocol.handler = { req in req.url!.path.hasSuffix("/vms") ? (200, #"[{"id":"1","name":"a"}]"#) : (500, "boom") }
        await #expect(throws: MachinaError.self) { _ = try await makeClient().load(spec) }                // every child failed: report it
    }

    @Test func fleetCloudRegistry() {
        let fc = FeatureRegistry.all.filter { $0.group == "Fleet Cloud" }
        #expect(fc.count >= 17)
        #expect(fc.filter { $0.create != nil }.map(\.id).sorted() == ["fc-flavors", "fc-instances", "fc-keypairs", "fc-projects", "fc-volumes"])
        #expect(fc.allSatisfy { $0.actions.allSatisfy { $0.method != "GET" } })
    }
}

extension ClientTests {
    @Test func deepLinksParseAndRoundTrip() {
        #expect(DeepLink(url: URL(string: "machina://vm/web-frontend-01")!) == .vm("web-frontend-01"))
        #expect(DeepLink(url: URL(string: "machina://vm/my%20vm")!) == .vm("my vm"))
        #expect(DeepLink(url: URL(string: "machina://page/k8s-pods")!) == .page("k8s-pods"))
        #expect(DeepLink(url: URL(string: "MACHINA://Fleet")!) == .fleet)
        #expect(DeepLink(url: URL(string: "https://vm/x")!) == nil)
        #expect(DeepLink(url: URL(string: "machina://vm")!) == nil)            // a VM link needs a name
        #expect(DeepLink(url: URL(string: "machina://nope/x")!) == nil)
        for l in [DeepLink.vm("a b"), .page("ai-cost"), .fleet, .tasks, .alerts] { #expect(DeepLink(url: l.url) == l) }
    }

    @Test func savedServersDedupeAndOrder() {
        let a = URL(string: "https://Host-A:5092")!, b = URL(string: "https://host-b:5092")!
        var l = ServerList.remembering([], url: a, username: "sus")
        l = ServerList.remembering(l, url: b, username: "sus")
        #expect(l.map { $0.url.host } == ["host-b", "Host-A"] || l.map { $0.url.host?.lowercased() } == ["host-b", "host-a"])
        l = ServerList.remembering(l, url: URL(string: "https://host-a:5092")!, username: "sus", name: "Lab")      // same server, other case: moves to the front
        #expect(l.count == 2 && l[0].name == "Lab")
        l = ServerList.remembering(l, url: a, username: "root")                                                  // other user = separate entry
        #expect(l.count == 3)
        #expect(ServerList.removing(l, id: l[0].id).count == 2)
        let json = try? JSONEncoder().encode(l)
        #expect((try? JSONDecoder().decode([ServerProfile].self, from: json ?? Data()))?.count == 3)
        // Profiles saved before `username` existed still decode.
        let old = #"[{"id":"7D1F3C52-0B0E-4C5B-8E43-1E6F2B1D9A10","name":"x","url":"https://h:5092"}]"#
        #expect((try? JSONDecoder().decode([ServerProfile].self, from: Data(old.utf8)))?.first?.username == nil)
    }
}
