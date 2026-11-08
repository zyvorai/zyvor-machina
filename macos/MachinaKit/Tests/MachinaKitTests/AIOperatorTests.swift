import Testing
import Foundation
@testable import MachinaKit

private func aiClient() -> MachinaClient {
    let cfg = URLSessionConfiguration.ephemeral
    cfg.protocolClasses = [MockProtocol.self]
    return MachinaClient(server: ServerProfile(name: "t", url: URL(string: "https://h:5092")!), session: URLSession(configuration: cfg))
}
private func aiBody(_ req: URLRequest) -> String {
    if let b = req.httpBody { return String(decoding: b, as: UTF8.self) }
    guard let st = req.httpBodyStream else { return "" }
    st.open(); defer { st.close() }
    var d = Data(); var buf = [UInt8](repeating: 0, count: 1024)
    while st.hasBytesAvailable { let n = st.read(&buf, maxLength: 1024); if n <= 0 { break }; d.append(buf, count: n) }
    return String(decoding: d, as: UTF8.self)
}
private func plan(_ s: String, role: String? = "operator") -> AIPlan {
    AIPlanParser.parse(try! JSONDecoder().decode(JSONValue.self, from: Data(s.utf8)), role: role)
}
private let uid1 = "11111111-1111-4111-8111-111111111111"
private let uid2 = "22222222-2222-4222-8222-222222222222"

extension ClientTests {
    @Test func aiParsesAPlanAndAlignsIds() {
        let p = plan(#"{"intent":"create_volume","summary":"Create a 10GiB volume","risk_score":3,"reply":"Prepared.","action_ids":["\#(uid1)"],"steps":[{"label":"Create 10GiB volume 'v'","action_type":"create_volume","review":"Provision","risk":"low"}]}"#)
        #expect(p.actions.count == 1 && p.dropped.isEmpty)
        #expect(p.actions[0].actionId == UUID(uuidString: uid1) && p.actions[0].isExecutable)
        #expect(p.actions[0].risk == "low" && p.riskScore == 3)
    }

    @Test func aiMisalignedOrBadIdsAreNotExecutable() {
        let two = #"[{"label":"a","action_type":"start_vm"},{"label":"b","action_type":"create_backup"}]"#
        #expect(plan(#"{"steps":\#(two),"action_ids":["\#(uid1)"]}"#).actions.allSatisfy { !$0.isExecutable })
        #expect(plan(#"{"steps":\#(two),"action_ids":["\#(uid1)","../../etc"]}"#).actions.map(\.isExecutable) == [true, false])
    }

    @Test func aiDropsUnknownActionTypes() {
        let p = plan(#"{"steps":[{"label":"x","action_type":"delete_all_vms"},{"label":"y","action_type":"start_vm"},{"label":"z"}],"action_ids":[]}"#)
        #expect(p.actions.map(\.actionType) == ["start_vm"])
        #expect(p.dropped.count == 2 && p.dropped.allSatisfy { $0.reason == "unknown action type" })
    }

    @Test func aiDropsAnActionNamingADisallowedPathOrVerb() {
        let steps = [
            #"{"label":"a","action_type":"start_vm","path":"/api/v1/vms/x/delete","method":"DELETE"}"#,
            #"{"label":"b","action_type":"start_vm","endpoint":"/api/v1/auth/users","method":"POST"}"#,
            #"{"label":"c","action_type":"start_vm","url":"https://evil.example/api/v1/ai/actions/\#(uid1)/execute"}"#,
            #"{"label":"d","action_type":"start_vm","method":"DELETE"}"#,
            #"{"label":"e","action_type":"start_vm","path":"/api/v1/ai/actions/\#(uid1)/execute","method":"POST"}"#,
            #"{"label":"f","action_type":"start_vm","path":["x"]}"#,
        ].joined(separator: ",")
        let p = plan(#"{"steps":[\#(steps)],"action_ids":[]}"#)
        #expect(p.actions.map(\.id) == [4])
        #expect(p.dropped.count == 5)
    }

    @Test func aiDropsWhatTheRoleCannotDo() {
        let s = #"{"steps":[{"label":"rm","action_type":"remove_stale_vm"},{"label":"go","action_type":"start_vm"}],"action_ids":[]}"#
        #expect(plan(s, role: "operator").actions.map(\.actionType) == ["start_vm"])
        #expect(plan(s, role: "operator").dropped.first?.reason == "needs the admin role")
        #expect(plan(s, role: "admin").actions.count == 2)
        #expect(plan(s, role: "readonly").actions.isEmpty && plan(s, role: nil).actions.isEmpty)
        #expect(plan(s, role: "readonly").dropped.count == 2)
    }

    @Test func aiRemoveNeedsATypedPhraseAndDestructiveFlag() {
        let p = AIAllowlist.policy(for: "remove_stale_vm")!
        #expect(p.destructive && p.adminOnly && p.typedPhrase == "REMOVE")
        #expect(AIAllowlist.policy(for: "stop_vm")!.destructive)
        #expect(AIAllowlist.policy(for: "rm -rf") == nil)
    }

    @Test func aiCleansModelTextAndCapsSteps() {
        #expect(AIPlanParser.clean("hi\u{07}\u{1B}[31m there\nok") == "hi[31m there\nok")
        #expect(AIPlanParser.clean(String(repeating: "a", count: 900)).count == AIPlanParser.maxText + 1)
        let many = (0..<20).map { _ in #"{"label":"s","action_type":"start_vm"}"# }.joined(separator: ",")
        let p = plan(#"{"steps":[\#(many)],"action_ids":[]}"#)
        #expect(p.actions.count == AIPlanParser.maxActions && p.dropped.count == 8)
    }

    @Test func aiGarbageAnswersGiveAnEmptyPlan() {
        #expect(plan("[]").actions.isEmpty)
        #expect(plan(#""just text""#).actions.isEmpty)
        #expect(plan(#"{"steps":"drop table","action_ids":5}"#).actions.isEmpty)
    }

    @Test func aiEndpointAllowlist() {
        #expect(AIAllowlist.permits(method: "POST", path: "/api/v1/ai/nl-ops"))
        #expect(AIAllowlist.permits(method: "POST", path: "/api/v1/platform/controller/api/v1/ai/actions/\(uid1)/execute"))
        #expect(AIAllowlist.permits(method: "GET", path: "/api/v1/ai/providers"))
        #expect(!AIAllowlist.permits(method: "DELETE", path: "/api/v1/ai/actions/\(uid1)/execute"))
        #expect(!AIAllowlist.permits(method: "POST", path: "/api/v1/ai/actions/not-a-uuid/execute"))
        #expect(!AIAllowlist.permits(method: "POST", path: "/api/v1/ai/actions/\(uid1)/execute/../../vms"))
        #expect(!AIAllowlist.permits(method: "POST", path: "/api/v1/vms/x/delete"))
        #expect(!AIAllowlist.permits(method: "POST", path: "/api/v1/ai/providers"))
    }

    @Test func aiExecuteEndpointIsRoleGatedAndBuiltByTheApp() throws {
        let p = plan(#"{"steps":[{"label":"go","action_type":"stop_vm"}],"action_ids":["\#(uid1)"]}"#)
        let a = p.actions[0]
        let e = try AIOperator.executeEndpoint(for: a, role: "operator")
        #expect(e.method == "POST" && e.viaController && e.resolvedPath == "/api/v1/platform/controller/api/v1/ai/actions/\(uid1)/execute")
        #expect(throws: AIOperatorError.self) { try AIOperator.executeEndpoint(for: a, role: "readonly") }
        var admin = a; admin.actionType = "remove_stale_vm"; admin.policy = AIAllowlist.policy(for: "remove_stale_vm")!
        #expect(throws: AIOperatorError.self) { try AIOperator.executeEndpoint(for: admin, role: "operator") }
        var forged = a; forged.actionType = "wipe"
        #expect(throws: AIOperatorError.self) { try AIOperator.executeEndpoint(for: forged, role: "admin") }
        var unqueued = a; unqueued.actionId = nil
        #expect(throws: AIOperatorError.self) { try AIOperator.executeEndpoint(for: unqueued, role: "admin") }
        #expect(try AIOperator.rejectEndpoint(for: a, role: "operator").path.hasSuffix("/reject"))
    }

    @Test func aiProviderStates() {
        func st(_ s: String) -> AIProviderState { AIProviderState.parse(try! JSONDecoder().decode(JSONValue.self, from: Data(s.utf8))) }
        #expect(!st("[]").isAvailable && st("[]").label.contains("no provider configured"))
        #expect(!st(#"[{"name":"x","enabled":false}]"#).isAvailable)
        #expect(st(#"[{"name":"a","kind":"openai","deployment_name":"m1","enabled":true},{"name":"b","kind":"ollama","deployment_name":"m2","enabled":true,"is_default":true}]"#)
                == .available(name: "b", kind: "ollama", model: "m2"))
        #expect(st(#"[{"name":"b","kind":"ollama","deployment_name":"m2","enabled":true}]"#).label == "b · m2")
    }

    @Test func aiRequestsCarryOnlyTheTypedText() async throws {
        let seen = Counter()
        MockProtocol.failWith = nil
        MockProtocol.handler = { req in
            seen.add("\(req.httpMethod ?? "") \(req.url!.path) \(aiBody(req))")
            if req.url!.path.hasSuffix("nl-ops") { return (200, #"{"steps":[],"action_ids":[],"reply":"","summary":""}"#) }
            return (200, #"{"reply":"All quiet.","deterministic":false,"agent_id":"ops"}"#)
        }
        let provider = AIProviderState.available(name: "p", kind: "k", model: "m")
        let a = try await AIOperator.ask("  how is the fleet?  ", client: aiClient(), role: "operator", provider: provider)
        #expect(a.reply == "All quiet." && a.source.contains("ops") && a.source.contains("p · m"))
        let all = seen.all()
        #expect(all.count == 2)
        #expect(all[0].contains(#""query":"how is the fleet?""#) && all[0].contains("nl-ops"))
        #expect(all[1].contains(#""message":"how is the fleet?""#))
        for line in all { #expect(!line.lowercased().contains("password") && !line.lowercased().contains("cookie")) }
    }

    @Test func aiNeverRunsAnythingOnItsOwn() async throws {
        let seen = Counter()
        MockProtocol.failWith = nil
        MockProtocol.handler = { req in
            seen.add(req.url!.path)
            return (200, #"{"steps":[{"label":"Stop","action_type":"stop_vm","review":"r","risk":"medium"}],"action_ids":["\#(uid1)"],"reply":"ok","summary":"s"}"#)
        }
        let a = try await AIOperator.ask("stop vm x", client: aiClient(), role: "admin", provider: .unknown)
        #expect(a.plan.actions.count == 1)
        #expect(seen.all().allSatisfy { !$0.hasSuffix("/execute") } && seen.all().count == 1)
    }

    @Test func aiReadonlyRoleIsRefusedBeforeAnyRequest() async {
        let seen = Counter()
        MockProtocol.failWith = nil
        MockProtocol.handler = { _ in seen.add("x"); return (200, "{}") }
        await #expect(throws: AIOperatorError.self) { try await AIOperator.ask("hi", client: aiClient(), role: "readonly", provider: .unknown) }
        #expect(seen.count("x") == 0)
    }

    @Test func aiProviderLookupFailureReadsAsUnavailable() async {
        MockProtocol.failWith = nil
        MockProtocol.handler = { _ in (404, "no") }
        let s = await AIOperator.providerState(aiClient())
        #expect(!s.isAvailable && s.label.hasPrefix("AI unavailable"))
    }

    @Test func aiHistoryScrubsSecretsAndHonoursOptOut() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("ai-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        let store = AIHistoryStore(url: dir.appendingPathComponent("h.json"))
        let msgs = [AIMessage(role: .user, text: "login with password=hunter2 and Bearer abcdefghijklmnop sk-ABCDEFGHIJKLMN"),
                    AIMessage(role: .assistant, text: "token: eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig fine", actionLabels: ["Start vm"])]
        store.save(msgs, enabled: true)
        let raw = try String(contentsOf: store.url, encoding: .utf8)
        #expect(!raw.contains("hunter2") && !raw.contains("abcdefghijklmnop") && !raw.contains("sk-ABCDEF") && !raw.contains("eyJhbGci"))
        #expect(store.load().count == 2 && store.load()[1].actionLabels == ["Start vm"])
        store.save(msgs, enabled: false)
        #expect(!FileManager.default.fileExists(atPath: store.url.path) && store.load().isEmpty)
        store.save((0..<260).map { AIMessage(role: .user, text: "m\($0)") }, enabled: true)
        #expect(store.load().count == AIHistoryStore.maxMessages)
    }
}
