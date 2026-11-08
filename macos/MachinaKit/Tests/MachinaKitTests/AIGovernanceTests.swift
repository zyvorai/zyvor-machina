import Testing
import Foundation
@testable import MachinaKit

private final class AIGMock: URLProtocol, @unchecked Sendable {
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
private func aigClient() -> MachinaClient {
    let cfg = URLSessionConfiguration.ephemeral; cfg.protocolClasses = [AIGMock.self]
    return MachinaClient(server: ServerProfile(name: "t", url: URL(string: "https://h:5092")!), session: URLSession(configuration: cfg))
}
private func j(_ s: String) -> JSONValue { try! JSONDecoder().decode(JSONValue.self, from: Data(s.utf8)) }
private let vmid = "7b6f1c52-1111-4222-8333-444455556666"
private func rec(_ extra: String = "") -> RightsizingRec {
    RightsizingRec(json: j(#"{"vm_id":"\#(vmid)","vm_name":"web1","action":"rightsize","current_memory_mib":8192,"suggested_memory_mib":2048,"savings_usd":12.5,"risk":"low","detail":"x"\#(extra)}"#))
}
private let planJSON = #"{"label":"Dev","review":"r","environment_type":"dev","vm_count":3,"vcpus_per_vm":4,"memory_gib_per_vm":8,"storage_gib":240,"estimated_monthly_usd":300.5,"gpu_required":false,"build_steps":["Create VMs","Join network"]}"#

@Suite(.serialized) struct AIGovernanceTests {
    @Test func roleGateIsAdminOnly() {
        #expect(AIGov.canWrite(role: "admin"))
        #expect(!AIGov.canWrite(role: "operator") && !AIGov.canWrite(role: "readonly") && !AIGov.canWrite(role: nil))
    }

    @Test func attributionParsesAndChartRowsSumToTotal() {
        let a = CostAttribution(json: j(#"{"total_monthly_usd":150,"unattributed_monthly_usd":30,"summary":"s","teams":[{"team":"a","vm_count":2,"vcpus":4,"memory_gib":8,"estimated_monthly_usd":100,"exposure_monthly_usd":0,"share_pct":66.6},{"team":"","vm_count":1,"vcpus":1,"memory_gib":1,"estimated_monthly_usd":20,"share_pct":13}]}"#))
        #expect(a.teams.count == 2 && a.teams[0].vmCount == 2)
        #expect(a.chartRows.map(\.name) == ["a", "Unattributed", "(untagged)"])
        #expect(a.chartRows.reduce(0) { $0 + $1.usd } == 150)
    }

    @Test func budgetIsReadOnlyAndParses() {
        let b = CostBudget(json: j(#"{"monthly_budget_usd":1000,"current_spend_usd":850,"predicted_spend_usd":900,"utilization_pct":85,"status":"warning","alerts":[{"severity":"warning","message":"m"}],"summary":"s"}"#))
        #expect(b.budgetUSD == 1000 && b.alerts == ["warning: m"])
        #expect(CostBudget.setNote.contains("read-only"))
    }

    @Test func frameworksListFailedControlsPerFramework() {
        let r = ComplianceFrameworks(json: j(#"{"summary":"s","frameworks":[{"framework":"CIS","score":70,"grade":"C","control_count":3,"failed_count":1}],"controls":[{"id":"a","framework":"CIS","title":"T","passed":false,"detail":"d"},{"id":"b","framework":"CIS","title":"U","passed":true,"detail":""},{"id":"c","framework":"PCI","title":"V","passed":false,"detail":""}]}"#))
        #expect(r.frameworks[0].failed == 1)
        #expect(r.failedControls(in: "CIS").map(\.id) == ["a"])
    }

    @Test func remediationDropsUnknownActionsAndIsPreviewOnly() {
        let r = ComplianceRemediation.parse(j(#"{"remediations":[{"id":"compliance-fix-1","framework":"CIS","control":"c","action":"enable_ha","label":"Fix\u0007 it","review":"rev","risk":"Low"},{"id":"x","action":"rm_rf","label":"bad"},{"id":"","action":"enable_ha"}]}"#))
        #expect(r.proposals.count == 1 && r.rejected == 2)
        #expect(r.proposals[0].label == "Fix it")
        #expect(ComplianceRemediation.applyNote.contains("Preview only"))
    }

    @Test func rightsizingApplyValidatesAndBuildsOneVMRequest() throws {
        let e = try RightsizingApply.endpoint(rec())
        #expect(e.method == "POST" && e.resolvedPath == "/api/v1/platform/controller/api/v1/vms/\(vmid)/memory")
        #expect(e.body?["memory_mb"] == .number(2048))
        #expect(RightsizingApply.phrase(rec()) == "web1")
        let ba = RightsizingApply.beforeAfter(rec(), vcpus: 2)
        #expect(ba[0].before == "2" && ba[0].after == "2" && ba[1].before == "8192 MiB" && ba[1].after == "2048 MiB")
        func bad(_ r: RightsizingRec) -> Bool { RightsizingApply.problem(r) != nil }
        var r = rec(); r.action = "power_off"; #expect(bad(r))
        r = rec(); r.vmID = "nope"; #expect(bad(r))
        r = rec(); r.suggestedMiB = 8192; #expect(bad(r))      // not a shrink
        r = rec(); r.suggestedMiB = 100; #expect(bad(r))       // below the floor
        r = rec(); r.currentMiB = 0; #expect(bad(r))
        #expect(throws: AIGov.ExportError.self) { try RightsizingApply.endpoint(r) }
    }

    @Test func digestScheduleValidationAndBody() throws {
        var s = DigestSchedule(enabled: true, frequency: "weekly", hourUTC: 6, weekday: 3)
        let e = try s.putEndpoint()
        #expect(e.method == "PUT" && e.path == "/api/v1/ai/digest/schedule" && e.viaController)
        #expect(e.body?["enabled"] == .bool(true) && e.body?["frequency"] == .string("weekly") && e.body?["hour_utc"] == .number(6) && e.body?["weekday"] == .number(3))
        #expect(s.confirmText.contains("Wednesday"))
        s.hourUTC = 24; #expect(s.problem != nil); #expect(throws: AIGov.ExportError.self) { try s.putEndpoint() }
        s = DigestSchedule(); s.frequency = "hourly"; #expect(s.problem != nil)
        s = DigestSchedule(); s.weekday = 8; #expect(s.problem != nil)
        let p = DigestSchedule(json: j(#"{"enabled":true,"frequency":"daily","hour_utc":9,"weekday":1,"last_sent_at":null}"#))
        #expect(p.enabled && p.hourUTC == 9 && p.lastSentAt == nil)
        #expect(AIGov.digestSend.method == "POST" && AIGov.digestSend.path == "/api/v1/ai/digest/send")
    }

    @Test func planIsValidatedBeforeItIsOffered() throws {
        let p = try EnvironmentIntent.validated(j(planJSON)).get()
        #expect(p.vmCount == 3 && p.steps == ["Create VMs", "Join network"])
        func refused(_ s: String) -> Bool { if case .failure = EnvironmentIntent.validated(j(s)) { return true }; return false }
        #expect(refused(planJSON.replacingOccurrences(of: #""vm_count":3"#, with: #""vm_count":5000"#)))
        #expect(refused(planJSON.replacingOccurrences(of: #""gpu_required":false"#, with: #""gpu_required":true"#)))
        #expect(refused(planJSON.replacingOccurrences(of: #""environment_type":"dev""#, with: #""environment_type":"dev; rm -rf /""#)))
        #expect(refused(planJSON.replacingOccurrences(of: #""vcpus_per_vm":4"#, with: #""vcpus_per_vm":0"#)))
        #expect(refused(#"{"unexpected":"shape"}"#))
        #expect(refused(#"[1,2]"#))
    }

    @Test func planTextIsStrippedOfControlCharacters() throws {
        let p = try EnvironmentIntent.validated(j(planJSON.replacingOccurrences(of: "Create VMs", with: #"Create\u001b[31m VMs"#))).get()
        #expect(!p.steps[0].unicodeScalars.contains { $0.value < 0x20 })
    }

    @Test func executeBodyIsFixedAndCapped() throws {
        let e = try EnvironmentIntent.executeEndpoint(query: "  dev env for 3  ", dryRun: false, maxVMs: 50)
        #expect(e.path == "/api/v1/ai/intent/environment/execute" && e.viaController)
        #expect(e.body?["dry_run"] == .bool(false) && e.body?["max_vms"] == .number(5) && e.body?["query"] == .string("dev env for 3"))
        #expect(try EnvironmentIntent.executeEndpoint(query: "x", dryRun: true, maxVMs: 0).body?["max_vms"] == .number(1))
        #expect(throws: AIGov.ExportError.self) { try EnvironmentIntent.executeEndpoint(query: "   ", dryRun: true, maxVMs: 1) }
        #expect(throws: AIGov.ExportError.self) { try EnvironmentIntent.planEndpoint(query: String(repeating: "a", count: 501)) }
        #expect(EnvironmentIntent.phrase(try! EnvironmentIntent.validated(j(planJSON)).get(), vms: 3) == "CREATE 3 VMS")
    }

    @Test func placementsKeepOnlyGeneratedNames() {
        let r = EnvironmentIntent.placements(j(#"{"vm_tasks":[{"name":"dev-dev-01","host":"h1"},{"name":"../etc","host":"h"},{"name":"Dev-dev-02","host":"h"}]}"#))
        #expect(r.map(\.name) == ["dev-dev-01"])
    }

    @Test func exportsUseRealRoutesAndRejectHTMLCsv() throws {
        #expect(AIGov.exports.contains { $0.path == "/api/v1/ai/cost/attribution/export.csv" })
        #expect(AIGov.exports.contains { $0.path == "/api/v1/ai/compliance/export" && $0.ext == "html" })
        let csv = try #require(AIGov.exports.first { $0.id == "cost" })
        #expect(throws: AIGov.ExportError.self) { try AIGov.validate(Data(), for: csv) }
        #expect(throws: AIGov.ExportError.self) { try AIGov.validate(Data("<!doctype html><html>".utf8), for: csv) }
        try AIGov.validate(Data("a,b\n1,2\n".utf8), for: csv)
        let html = try #require(AIGov.exports.first { $0.id == "compliance" })
        try AIGov.validate(Data("<!doctype html><html>".utf8), for: html)
    }

    @Test func clientFetchesExportBytesAndRightsizingCall() async throws {
        AIGMock.handler = { req in
            if req.url!.path.hasSuffix("/cost/export.csv") { return (200, "vm,usd\na,1\n") }
            if req.url!.path.hasSuffix("/cost") { return (200, #"{"estimated_monthly_usd":5}"#) }
            return (404, "")
        }
        let c = aigClient()
        let csv = try await c.exportData(AIGov.exports.first { $0.id == "cost" }!)
        #expect(String(decoding: csv, as: UTF8.self).hasPrefix("vm,usd"))
        let js = try await c.exportData(AIGov.exports.first { $0.id == "cost-json" }!)
        #expect(String(decoding: js, as: UTF8.self).contains("estimated_monthly_usd"))
    }

    @Test func pageIsRegistered() {
        #expect(FeatureRegistry.spec(AIGov.pageID)?.group == "Insight")
    }
}
