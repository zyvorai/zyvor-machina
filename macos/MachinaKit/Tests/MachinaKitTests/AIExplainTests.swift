import Testing
import Foundation
@testable import MachinaKit

private final class AIMock: URLProtocol, @unchecked Sendable {
    nonisolated(unsafe) static var handler: (@Sendable (URLRequest, Data) -> (Int, String))?
    nonisolated(unsafe) static var seen: [(String, Data)] = []
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        var body = request.httpBody ?? Data()
        if body.isEmpty, let s = request.httpBodyStream { s.open(); var buf = [UInt8](repeating: 0, count: 65536); let n = s.read(&buf, maxLength: buf.count); if n > 0 { body = Data(buf[0..<n]) }; s.close() }
        Self.seen.append((request.url!.path, body))
        let (code, text) = Self.handler?(request, body) ?? (500, "")
        client?.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: code, httpVersion: nil, headerFields: nil)!, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(text.utf8)); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

private func client() -> MachinaClient {
    let cfg = URLSessionConfiguration.ephemeral; cfg.protocolClasses = [AIMock.self]
    AIMock.seen = []
    return MachinaClient(server: ServerProfile(name: "t", url: URL(string: "https://h:5092")!), session: URLSession(configuration: cfg))
}
private func bodyJSON(_ d: Data) -> JSONValue { (try? JSONDecoder().decode(JSONValue.self, from: d)) ?? .null }

@Suite(.serialized) struct AIExplainTests {
    @Test func vmExplainBodyHasOnlyIdAndName() {
        let b = AIExplainRequest.vm(id: "u-1", name: "web-1").endpoint
        #expect(b.path == "/api/v1/ai/explain" && b.viaController && b.method == "POST")
        #expect(b.body == .object(["screen": .string("vm_detail"), "object_ref": .object(["vm_id": .string("u-1"), "vm_name": .string("web-1")])]))
        #expect(b.resolvedPath == "/api/v1/platform/controller/api/v1/ai/explain")
    }

    @Test func securityEventDropsSecretsAndCutsLongText() {
        let ev: JSONValue = .object(["kind": .string("exec"), "password": .string("hunter2"), "api_token": .string("x"),
                                     "nested": .object(["Cookie": .string("c"), "ok": .string("fine")]), "blob": .string(String(repeating: "a", count: 900))])
        let body = AIExplainRequest.securityEvent(fields: ev, hostID: "h1").endpoint.body!
        let sent = String(decoding: try! JSONEncoder().encode(body), as: UTF8.self)
        #expect(!sent.contains("hunter2") && !sent.contains("Cookie") && !sent.contains("api_token"))
        #expect(sent.contains("fine") && sent.contains("h1"))
        #expect(body["event"]?["blob"]?.text.count == 500)
    }

    @Test func otherEndpointsAndBodies() {
        #expect(AIExplainRequest.firewall(targetID: "t", question: "why").endpoint.path == "/api/v1/ai/firewall/explain")
        #expect(AIExplainRequest.firewall(targetID: "t", question: nil).endpoint.body == .object(["target_id": .string("t")]))
        let n = AIExplainRequest.network(vmA: "a", vmB: "b", port: 443).endpoint
        #expect(n.path == "/api/v1/ai/network/explain" && n.body?["port"] == .number(443))
        #expect(AIExplainRequest.alert(id: "1", rule: "r", severity: "high", message: "m").runbookEndpoint?.path == "/api/v1/ai/runbook")
        #expect(AIExplainRequest.firewall(targetID: "t", question: nil).runbookEndpoint == nil)
    }

    @Test func rowMapping() {
        let row = ResourceRow(id: "x", values: .object(["a": .string("b")]))
        #expect(AIExplainRequest.forRow(specID: "fw-targets", row: row) == .firewall(targetID: "x", question: nil))
        #expect(AIExplainRequest.forRow(specID: "bpf-events", row: row) != nil)
        #expect(AIExplainRequest.forRow(specID: "users", row: row) == nil)
    }

    @Test func hostileTextIsCleaned() {
        let evil = "ok\u{202E}gpj.exe\u{0007}\u{200B}\r\n![x](https://evil.example/p.png) [a](javascript:1) <img src=x>"
        let c = AIText.clean(evil)
        #expect(!c.contains("\u{202E}") && !c.contains("\u{0007}") && !c.contains("\u{200B}") && !c.contains("\r"))
        let md = AIText.markdownEscaped(evil)
        #expect(md.contains("\\!\\[x\\]\\(") && md.contains("\\<img"))
        #expect(!md.contains("![x]") && !md.replacingOccurrences(of: "\\<", with: "").contains("<"))
        #expect(AIText.clean(String(repeating: "z", count: 30_000)).count <= AIText.maxLength + 1)
        let f = AIText.fenced("a ``` b\n```` c")
        #expect(f.hasPrefix("`````\n") && f.hasSuffix("\n`````"))
    }

    @Test func explainRoundTripAndRunbookSteps() async throws {
        let c = client()
        AIMock.handler = { req, _ in
            req.url!.path.hasSuffix("/ai/runbook") ? (200, #"{"steps":["Check disk","Restart‮ svc"],"commands":["df -h"]}"#) : (200, #"{"explanation":"Disk is full."}"#)
        }
        let e = try await c.aiExplain(.alert(id: "9", rule: "disk", severity: "critical", message: "full"))
        #expect(e.text == "Disk is full." && e.steps == ["Check disk", "Restart svc"] && e.commands == ["df -h"])
        #expect(AIMock.seen.count == 2)
        let first = bodyJSON(AIMock.seen[0].1)
        #expect(first["screen"] == .string("notification") && first["object_ref"]?["alert_id"] == .string("9"))
        #expect(e.markdown().contains("## Steps") && e.markdown().contains("Commands (review"))
    }

    @Test func runbookFailureDoesNotFailExplain() async throws {
        let c = client()
        AIMock.handler = { req, _ in req.url!.path.hasSuffix("/ai/runbook") ? (500, "boom") : (200, #"{"explanation":"E"}"#) }
        let e = try await c.aiExplain(.failedTask(id: "t", operation: "vm.backup", status: "failed", message: "x"))
        #expect(e.text == "E" && e.steps.isEmpty)
    }

    @Test func securityAndFirewallAndNetworkParsing() async throws {
        let c = client()
        AIMock.handler = { req, _ in
            let p = req.url!.path
            if p.hasSuffix("security/explain-event") { return (200, #"{"explanation":"X","risk":"high","recommendation":"isolate"}"#) }
            if p.hasSuffix("firewall/explain") { return (200, #"{"target":"t","risk":"med","evidence":["e1"],"recommendation":"r","summary":"S"}"#) }
            return (200, #"{"can_reach":false,"explanation":"blocked","hops":["a","b"],"remediation":"open 443"}"#)
        }
        let s = try await c.aiExplain(.securityEvent(fields: .object(["k": .string("v")]), hostID: nil))
        #expect(s.facts.map(\.value) == ["high", "isolate"])
        let f = try await c.aiExplain(.firewall(targetID: "t", question: nil))
        #expect(f.text == "S" && f.steps == ["e1"])
        let n = try await c.aiExplain(.network(vmA: "a", vmB: "b", port: nil))
        #expect(n.facts.first == AIFact(label: "Reachable", value: "no") && n.steps == ["a", "b"])
    }

    @Test func unavailableForbiddenAndEmptyStates() async {
        let c = client()
        func failure(_ f: () async throws -> Void) async -> AIFailure? {
            do { try await f(); return nil } catch { return error as? AIFailure }
        }
        AIMock.handler = { _, _ in (503, "no llm") }
        var r = await failure { _ = try await c.aiExplain(.vm(id: nil, name: "v")) }
        if case .unavailable = r {} else { Issue.record("expected unavailable, got \(String(describing: r))") }
        AIMock.handler = { _, _ in (403, "no") }
        r = await failure { _ = try await c.aiExplain(.vm(id: nil, name: "v")) }
        #expect(r == .forbidden)
        AIMock.handler = { _, _ in (200, "{}") }
        r = await failure { _ = try await c.aiExplain(.vm(id: nil, name: "v")) }
        if case .unavailable = r {} else { Issue.record("expected unavailable for empty answer") }
        AIMock.handler = { _, _ in (200, "<!DOCTYPE html><html>") }
        r = await failure { _ = try await c.knowledgeSearch("x") }
        if case .unavailable = r {} else { Issue.record("expected unavailable for HTML") }
        r = await failure { _ = try await c.knowledgeSearch("   ") }
        #expect(r == .emptyQuery)
    }

    @Test func knowledgeSearchDiagnoseRunbook() async throws {
        let c = client()
        AIMock.handler = { req, _ in
            let p = req.url!.path
            if p.hasSuffix("knowledge/search") { return (200, #"{"query":"q","hits":[{"kind":"runbook","id":"1","title":"Disk\u0007 full","snippet":"clean it","score":0.9}]}"#) }
            if p.hasSuffix("fleet/diagnose") { return (200, #"{"query":"q","summary":"S","zyra_status":"ok","linux_summary":"L","hypotheses":[{"title":"H","confidence":80,"evidence":"E","action":"A"}]}"#) }
            if p.hasSuffix("knowledge/diagnose") { return (200, #"{"summary":"S","hypotheses":[{"title":"H","confidence":0.4,"evidence":"E","action":"A"}]}"#) }
            return (200, #"{"query":"q","diagnosis_summary":"D","runbook_title":"Fix **disk**","steps":["one","two"],"commands":["ls"],"summary":"Sum"}"#)
        }
        let h = try await c.knowledgeSearch("  disk  ")
        #expect(h.count == 1 && h[0].title == "Disk full")
        #expect(bodyJSON(AIMock.seen[0].1) == .object(["query": .string("disk")]))
        let d = try await c.knowledgeDiagnose("slow", fleet: true)
        #expect(d.hypotheses[0].confidence == 0.8 && d.extra.count == 2)
        let d2 = try await c.knowledgeDiagnose("slow")
        #expect(d2.hypotheses[0].confidence == 0.4 && d2.markdown().contains("40%"))
        let r = try await c.knowledgeRunbook("disk")
        #expect(r.steps == ["one", "two"] && r.plain().contains("1. one"))
        #expect(r.markdown().hasPrefix("# Fix \\*\\*disk\\*\\*") && r.markdown().contains("```\nls\n```"))
    }

    @Test func queryIsCapped() throws {
        #expect(try AIQuery.validated(String(repeating: "a", count: 5000)).count == AIQuery.maxLength)
    }
}
