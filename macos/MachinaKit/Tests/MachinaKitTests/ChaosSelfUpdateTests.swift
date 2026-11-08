import Testing
import Foundation
@testable import MachinaKit

/// Own mock (the shared MockProtocol handler is global and the suites run in parallel).
private final class ChaosMock: URLProtocol, @unchecked Sendable {
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

private func chaosClient() -> MachinaClient {
    let cfg = URLSessionConfiguration.ephemeral
    cfg.protocolClasses = [ChaosMock.self]
    return MachinaClient(server: ServerProfile(name: "t", url: URL(string: "https://h:5092")!), session: URLSession(configuration: cfg))
}

private let vmA = "11111111-1111-1111-1111-111111111111"

private func goodDraft() -> ChaosDraft {
    var d = ChaosDraft(); d.name = "web-latency"; d.targets = [vmA]
    d.steps = [.latency(delayMs: 100, jitterMs: 10, secs: 30)]; d.probes = [ChaosDraft.tcpProbe("10.0.0.5:443")]
    return d
}

@Suite(.serialized) struct ChaosOpsTests {
    @Test func newDraftTargetsNothing() {
        let d = ChaosDraft()
        #expect(d.targets.isEmpty)
        #expect(d.validate() != nil)
        var named = d; named.name = "x"; named.steps = [.kill(recoverSecs: 60)]
        #expect(named.validate()?.contains("target") == true)   // never defaults to all VMs
    }

    @Test func validationMirrorsServer() {
        var d = goodDraft(); #expect(d.validate() == nil)
        d.steps = [.latency(delayMs: 0, jitterMs: 0, secs: 5)]; #expect(d.validate() != nil)
        d.steps = [.loss(pct: 101, secs: 5)]; #expect(d.validate() != nil)
        d.steps = [.partition(cidrs: [], peers: [], secs: 5)]; #expect(d.validate() != nil)
        d.steps = [.disk(readIops: 0, writeIops: 0, secs: 5)]; #expect(d.validate() != nil)
        d.steps = [.kill(recoverSecs: 5)]; #expect(d.validate() != nil)
        d.steps = [.latency(delayMs: 50, jitterMs: 0, secs: 3600)]; #expect(d.validate()?.contains("3600") == true)  // plus baseline/recovery
        d = goodDraft(); d.windowSecs = 2; #expect(d.validate() != nil)
        d = goodDraft(); d.targets = (0..<21).map { _ in vmA }; #expect(d.validate() != nil)
    }

    @Test func bodyShape() throws {
        let b = try goodDraft().body()
        #expect(b["name"] == .string("web-latency"))
        let spec = try #require(b["spec"])
        #expect(spec["targets"] == .array([.string(vmA)]))
        #expect(spec["steps"]?.array?.first?["kind"] == .string("latency"))
        #expect(spec["steps"]?.array?.first?["delay_ms"] == .number(100))
        #expect(spec["abort"]?["window_secs"] == .number(20))
        #expect(throws: ResourceCreate.Invalid.self) { try ChaosDraft().body() }
    }

    @Test func roundTripFromRow() throws {
        let d = goodDraft()
        let again = ChaosDraft(row: try d.body())
        #expect(again == d)
    }

    @Test func blastRadiusNamesTargetsAndScope() {
        let t = goodDraft().blastRadius(vmNames: [vmA: "web-1"])
        #expect(t.contains("web-1")); #expect(t.contains("Injects real faults")); #expect(t.contains("Latency 100±10 ms"))
        var sim = goodDraft(); sim.steps = [.hostFailure(hostID: vmA)]
        #expect(sim.blastRadius().contains("Simulation only"))
        var noProbe = goodDraft(); noProbe.probes = []
        #expect(noProbe.blastRadius().contains("abort by hand"))
    }

    @Test func requestsHitControllerProxy() async throws {
        let seen = Counter()
        ChaosMock.handler = { req in seen.add("\(req.httpMethod ?? "") \(req.url!.path)"); return (200, "{}") }
        let c = chaosClient()
        _ = try await c.call(ChaosAPI.create(goodDraft()))
        _ = try await c.call(ChaosAPI.update(id: "e1", goodDraft()))
        _ = try await c.call(ChaosAPI.delete(id: "e1"))
        _ = try await c.call(ChaosAPI.run(id: "e1", name: "web-latency"))
        _ = try await c.call(ChaosAPI.abort(runID: "r1"))
        #expect(seen.count("POST /api/v1/platform/controller/api/v1/chaos/experiments") == 1)
        #expect(seen.count("PUT /api/v1/platform/controller/api/v1/chaos/experiments/e1") == 1)
        #expect(seen.count("DELETE /api/v1/platform/controller/api/v1/chaos/experiments/e1") == 1)
        #expect(seen.count("POST /api/v1/platform/controller/api/v1/chaos/experiments/e1/run") == 1)
        #expect(seen.count("POST /api/v1/platform/controller/api/v1/chaos/runs/r1/abort") == 1)
        #expect(ChaosAPI.run(id: "e1", name: "n").body?["confirm"] == .string("n"))
    }

    @Test func destructiveRegistryActionsAreAdminOnly() {
        let exp = FeatureRegistry.spec("chaos-experiments")!
        #expect(exp.accessory == .chaosEditor)
        #expect(exp.actions.allSatisfy { $0.adminOnly })
        #expect(FeatureRegistry.spec("chaos-runs")!.actions.allSatisfy { $0.adminOnly })
    }
}

@Suite(.serialized) struct SelfUpdateTests {
    private let hash = String(repeating: "ab", count: 32)

    @Test func hashCompare() throws {
        #expect(SelfUpdate.hashesMatch(hash.uppercased(), " \(hash)\n"))
        #expect(!SelfUpdate.hashesMatch(hash, String(repeating: "cd", count: 32)))
        #expect(!SelfUpdate.hashesMatch("abc", "abc"))
        let f = FileManager.default.temporaryDirectory.appendingPathComponent("h-\(UUID().uuidString)")
        try Data("hello".utf8).write(to: f); defer { try? FileManager.default.removeItem(at: f) }
        #expect(try SelfUpdate.sha256Hex(of: f) == "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
    }

    @Test func notesParsing() {
        let other = String(repeating: "cd", count: 32)
        let notes = "Changes\n\nSHA-256:\n`Machina-1.0.dmg`  \(hash)\n`Other.zip` \(other)"
        #expect(SelfUpdate.expectedSHA256(notes: notes, assetName: "Machina-1.0.dmg") == hash)
        #expect(SelfUpdate.expectedSHA256(notes: "sha256: \(hash.uppercased())", assetName: "Machina.dmg") == hash)
        #expect(SelfUpdate.expectedSHA256(notes: "no checksum here", assetName: "m.dmg") == nil)
        #expect(SelfUpdate.expectedSHA256(notes: "SHA256 \(hash)\nSHA256 \(other)", assetName: "m.dmg") == nil)   // ambiguous: never guess
        #expect(SelfUpdate.expectedSHA256(notes: nil, assetName: nil) == nil)
    }

    @Test func parsedReleaseCarriesNotes() throws {
        let json = #"{"tag_name":"v0.4.0","html_url":"https://github.com/x","body":"sha256 \#(hash)","assets":[{"name":"M.dmg","browser_download_url":"https://github.com/m.dmg"}]}"#
        let rel = try #require(UpdateChecker.parse(Data(json.utf8)))
        #expect(rel.dmgName == "M.dmg")
        #expect(SelfUpdate.expectedSHA256(notes: rel.notes, assetName: rel.dmgName) == hash)
    }

    @Test func teamComparison() {
        let out = "Identifier=dev.zyvor.machina\nAuthority=Developer ID Application: Zyvor (ABCDE12345)\nTeamIdentifier=ABCDE12345\n"
        #expect(SelfUpdate.teamID(fromCodesign: out) == "ABCDE12345")
        #expect(SelfUpdate.teamID(fromCodesign: "TeamIdentifier=not set") == nil)
        #expect(SelfUpdate.isDeveloperID(codesign: out))
        #expect(!SelfUpdate.isDeveloperID(codesign: "Authority=Apple Development: x"))
        #expect(SelfUpdate.sameTeam(running: "ABCDE12345", candidate: "ABCDE12345"))
        #expect(!SelfUpdate.sameTeam(running: "ABCDE12345", candidate: "ZZZZZ99999"))
        #expect(!SelfUpdate.sameTeam(running: nil, candidate: nil))      // unsigned never matches unsigned
        #expect(!SelfUpdate.sameTeam(running: nil, candidate: "ABCDE12345"))
    }

    @Test func versionAndURLPolicy() {
        #expect(SelfUpdate.isNewer(offered: "0.10.0", current: "0.9.0"))
        #expect(!SelfUpdate.isNewer(offered: "0.9.0", current: "0.9.0"))
        #expect(!SelfUpdate.isNewer(offered: "junk", current: "0.9.0"))
        #expect(SelfUpdate.downloadAllowed(URL(string: "https://github.com/a/b/releases/download/x/M.dmg")!))
        #expect(SelfUpdate.downloadAllowed(URL(string: "https://objects.githubusercontent.com/x")!))
        #expect(!SelfUpdate.downloadAllowed(URL(string: "http://github.com/x")!))
        #expect(!SelfUpdate.downloadAllowed(URL(string: "https://evilgithub.com/x")!))
        #expect(!SelfUpdate.downloadAllowed(URL(string: "https://github.com.evil.io/x")!))
    }

    @Test func downloadSessionHasNoCredentials() {
        let c = SelfUpdate.downloadSession().configuration
        #expect(c.urlCredentialStorage == nil); #expect(c.httpCookieStorage == nil); #expect(!c.httpShouldSetCookies)
    }

    private func makeApp(_ dir: URL, name: String, marker: String) throws -> URL {
        let app = dir.appendingPathComponent(name)
        try FileManager.default.createDirectory(at: app.appendingPathComponent("Contents"), withIntermediateDirectories: true)
        try Data(marker.utf8).write(to: app.appendingPathComponent("Contents/marker"))
        return app
    }
    private func tmp() throws -> URL {
        let d = FileManager.default.temporaryDirectory.appendingPathComponent("su-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: d, withIntermediateDirectories: true); return d
    }

    @Test func replaceAppSwapsAndCleansUp() throws {
        let root = try tmp(); defer { try? FileManager.default.removeItem(at: root) }
        let installDir = root.appendingPathComponent("Applications"); try FileManager.default.createDirectory(at: installDir, withIntermediateDirectories: true)
        let old = try makeApp(installDir, name: "Machina.app", marker: "old")
        let new = try makeApp(root, name: "Machina.app", marker: "new")
        try SelfUpdate.replaceApp(at: old, with: new)
        #expect(try String(contentsOf: old.appendingPathComponent("Contents/marker"), encoding: .utf8) == "new")
        #expect(try FileManager.default.contentsOfDirectory(atPath: installDir.path) == ["Machina.app"])   // no leftovers
    }

    @Test func replaceAppMissingSourceLeavesOldAlone() throws {
        let root = try tmp(); defer { try? FileManager.default.removeItem(at: root) }
        let old = try makeApp(root, name: "Machina.app", marker: "old")
        #expect(throws: SelfUpdate.Failure.self) { try SelfUpdate.replaceApp(at: old, with: root.appendingPathComponent("Nope.app")) }
        #expect(try String(contentsOf: old.appendingPathComponent("Contents/marker"), encoding: .utf8) == "old")
    }

    @Test func replaceAppRollsBackWhenInstallFails() throws {
        let root = try tmp(); defer { try? FileManager.default.removeItem(at: root); }
        let installDir = root.appendingPathComponent("Applications"); try FileManager.default.createDirectory(at: installDir, withIntermediateDirectories: true)
        let old = try makeApp(installDir, name: "Machina.app", marker: "old")
        let new = try makeApp(root, name: "Machina.app", marker: "new")
        final class FailingFM: FileManager, @unchecked Sendable {
            override func moveItem(at src: URL, to dst: URL) throws {
                if src.lastPathComponent.contains(".new-") { throw CocoaError(.fileWriteNoPermission) }
                try super.moveItem(at: src, to: dst)
            }
        }
        #expect(throws: SelfUpdate.Failure.self) { try SelfUpdate.replaceApp(at: old, with: new, fm: FailingFM()) }
        #expect(try String(contentsOf: old.appendingPathComponent("Contents/marker"), encoding: .utf8) == "old")
        #expect(try FileManager.default.contentsOfDirectory(atPath: installDir.path) == ["Machina.app"])
    }
}
