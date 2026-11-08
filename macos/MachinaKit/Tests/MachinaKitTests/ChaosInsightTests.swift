import Testing
import Foundation
@testable import MachinaKit

private final class RunMock: URLProtocol, @unchecked Sendable {
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

private let vmA = "11111111-1111-1111-1111-111111111111"
private let vmB = "22222222-2222-2222-2222-222222222222"

private func draft() -> ChaosDraft {
    var d = ChaosDraft(); d.name = "split"; d.targets = [vmA]
    d.steps = [.partition(cidrs: ["10.0.0.0/24"], peers: [vmB], secs: 20), .loss(pct: 10, secs: 10)]
    d.probes = [ChaosDraft.tcpProbe("10.0.0.5:443")]
    return d
}

private let runJSON = """
{"run":{"id":"r1","experiment_id":"e1","status":"completed","started_by":"sus","started_at":"2026-01-01 10:00:00","finished_at":"2026-01-01 10:01:00","abort_reason":"",
"report":{"verdict":"degraded","findings":["step 1 lost 60% of probes"],"probes":["tcp 10.0.0.5:443"],"targets":["web-1"],"duration_s":60.0,
"phases":[{"name":"Baseline","kind":"baseline","started_s":0,"ended_s":15,"samples":15,"ok":15,"success_pct":100.0,"p50_ms":4,"p95_ms":9},
{"name":"1. Partition","kind":"step","started_s":15,"ended_s":35,"samples":20,"ok":8,"success_pct":40.0,"notes":["n"]},
{"name":"2. Loss","kind":"step","started_s":35,"ended_s":45,"samples":10,"ok":10,"success_pct":100.0,"error":"fault not applied"}]}},"live":null}
"""

@Suite(.serialized) struct ChaosInsightTests {
    @Test func peersValidatedAndSerialized() throws {
        let d = draft()
        #expect(d.validate() == nil)
        let steps = try d.body()["spec"]?["steps"]?.array
        #expect(steps?[0]["peers"]?.array?.first == .string(vmB))
        var bad = d; bad.steps = [.partition(cidrs: [], peers: ["not-a-uuid"], secs: 5)]
        #expect(bad.validate()?.contains("peer VMs") == true)
        #expect(ChaosDraft(row: try d.body()) == d)
    }

    @Test func runDetailParsesPhasesAndVerdicts() {
        let v = try! JSONDecoder().decode(JSONValue.self, from: Data(runJSON.utf8))
        let r = ChaosRunDetail(json: v)
        #expect(r.status == "completed"); #expect(r.verdict == "degraded"); #expect(!r.canAbort)
        #expect(r.phases.count == 3)
        #expect(r.phases[0].passed()); #expect(!r.phases[1].passed()); #expect(!r.phases[2].passed())   // 40% < 50; error
        #expect(r.phases[1].passed(minSuccessPct: 30))
        #expect(r.passedPhases == 1); #expect(r.failedPhases == 2)
        #expect(r.findings.count == 1)
    }

    @Test func liveRunUsesLivePhasesAndCanAbort() {
        let v: JSONValue = .object(["run": .object(["id": .string("r2"), "status": .string("running"), "report": .null]),
                                    "live": .object(["phase": .number(1), "phases": .array([.object(["name": .string("Baseline"), "ok": .number(5), "samples": .number(5)])])])])
        let r = ChaosRunDetail(json: v)
        #expect(r.canAbort); #expect(r.livePhase == 1); #expect(r.phases.count == 1); #expect(r.finishedAt == nil)
    }

    @Test func duplicateClearsTargetsAndNamesCopy() {
        let c = ChaosPorting.duplicate(draft())
        #expect(c.name == "split (copy)"); #expect(c.targets.isEmpty); #expect(c.steps == draft().steps)
        var long = draft(); long.name = String(repeating: "a", count: 64)
        #expect(ChaosPorting.duplicate(long).name.count == 64)
    }

    @Test func exportImportRoundTripDropsTargets() throws {
        let data = try ChaosPorting.export(draft())
        let text = String(decoding: data, as: UTF8.self)
        #expect(!text.contains(vmA))
        let back = try ChaosPorting.importDraft(data)
        #expect(back.targets.isEmpty)
        #expect(back.steps == draft().steps); #expect(back.name == "split"); #expect(back.probes == draft().probes)
        #expect(back.validate() != nil)   // not runnable or savable until targets are picked
    }

    @Test func importRejectsBadInput() throws {
        func fails(_ s: String) -> Bool { (try? ChaosPorting.importDraft(Data(s.utf8))) == nil }
        #expect(fails("[1,2]")); #expect(fails("not json")); #expect(fails("{\"name\":\"x\"}"))
        #expect(fails("{\"name\":\"x\",\"spec\":{\"steps\":[{\"kind\":\"explode\"}]}}"))
        #expect(fails("{\"name\":\"\",\"spec\":{\"steps\":[{\"kind\":\"kill\",\"recover_secs\":60}]}}"))
        #expect(fails("{\"name\":\"x\",\"spec\":{\"steps\":[{\"kind\":\"latency\",\"delay_ms\":99999,\"secs\":5}]}}"))
        #expect(fails("{\"name\":\"x\",\"spec\":{\"steps\":[]}}"))
        #expect(throws: ChaosPorting.ImportError.self) { try ChaosPorting.importDraft(Data(count: ChaosPorting.maxBytes + 1)) }
        // controller-shaped JSON with targets imports with the targets removed
        let ok = "{\"name\":\"x\",\"spec\":{\"targets\":[\"\(vmA)\"],\"steps\":[{\"kind\":\"kill\",\"recover_secs\":60}]}}"
        #expect((try? ChaosPorting.importDraft(Data(ok.utf8)))?.targets.isEmpty == true)
    }

    @Test func runEndpointsAndRegistry() async throws {
        let cfg = URLSessionConfiguration.ephemeral; cfg.protocolClasses = [RunMock.self]
        let c = MachinaClient(server: ServerProfile(name: "t", url: URL(string: "https://h:5092")!), session: URLSession(configuration: cfg))
        RunMock.handler = { req in
            #expect(req.httpMethod == "GET")
            #expect(req.url!.path == "/api/v1/platform/controller/api/v1/chaos/runs/r1")
            return (200, runJSON)
        }
        let json = try await c.call(ChaosAPI.getRun(id: "r1"))
        #expect(ChaosRunDetail(json: json).phases.count == 3)
        #expect(FeatureRegistry.spec("chaos-runs")!.accessory == .chaosRuns)
    }
}
