import Foundation

/// AI cost, compliance, right-sizing, digest and intent features beyond the read-only pages.
/// Server facts this file is built on (controller `api/ai.rs`):
/// - `GET ai/cost/attribution`, `GET ai/cost/budget` (derived and read-only: there is NO route to set a budget),
///   `GET ai/compliance/frameworks`, `GET ai/compliance/remediate` (proposals only: there is NO apply route),
///   `GET ai/rightsizing/report`, `GET|PUT ai/digest/schedule` (PUT admin), `POST ai/digest/send` (admin),
///   `POST ai/intent/environment` (a plan from a sentence), `POST ai/intent/environment/execute` (admin when not a dry run).
/// - Exports are `ai/cost/export.csv`, `ai/capacity/export.csv`, `ai/cost/attribution/export.csv` and `ai/compliance/export` (HTML).
/// - A right-sizing recommendation is applied through the existing `POST vms/{id}/memory {memory_mb}`.
public enum AIGov {
    static let base = "/api/v1/ai"
    public static let attribution = Endpoint.controllerGet(base + "/cost/attribution")
    public static let budget = Endpoint.controllerGet(base + "/cost/budget")
    public static let frameworks = Endpoint.controllerGet(base + "/compliance/frameworks")
    public static let remediate = Endpoint.controllerGet(base + "/compliance/remediate")
    public static let rightsizing = Endpoint(  "GET", base + "/rightsizing/report", controller: true, timeout: 60)
    public static let digestSchedule = Endpoint.controllerGet(base + "/digest/schedule")
    public static let digestSend = Endpoint("POST", base + "/digest/send", controller: true, timeout: 60)

    /// Writes that may only be made by the admin role (the server enforces the same).
    public static func canWrite(role: String?) -> Bool { role == "admin" }
    public static let adminOnlyNote = "Needs the admin role"

    /// The sidebar entry; the app shows its own view for this id instead of a list.
    public static let pageID = "ai-governance"
    public static let specs: [ResourceSpec] = [
        ResourceSpec(pageID, "AI governance", group: "Insight", icon: "scalemass", endpoint: frameworks, layout: .keyValue),
    ]

    // MARK: Exports

    public struct Export: Identifiable, Equatable, Sendable {
        public let id: String
        public let title: String
        public let path: String
        public let ext: String
        /// JSON exports are the page's JSON saved as pretty text; the rest are the server's own file.
        public let raw: Bool
        public var fileName: String { "machina-\(id).\(ext)" }
    }
    public static let exports: [Export] = [
        .init(id: "cost", title: "Cost (CSV)", path: base + "/cost/export.csv", ext: "csv", raw: true),
        .init(id: "cost-json", title: "Cost (JSON)", path: base + "/cost", ext: "json", raw: false),
        .init(id: "capacity", title: "Capacity (CSV)", path: base + "/capacity/export.csv", ext: "csv", raw: true),
        .init(id: "capacity-json", title: "Capacity (JSON)", path: base + "/capacity", ext: "json", raw: false),
        .init(id: "attribution", title: "Cost attribution (CSV)", path: base + "/cost/attribution/export.csv", ext: "csv", raw: true),
        .init(id: "attribution-json", title: "Cost attribution (JSON)", path: base + "/cost/attribution", ext: "json", raw: false),
        .init(id: "compliance", title: "Compliance report (HTML)", path: base + "/compliance/export", ext: "html", raw: true),
        .init(id: "frameworks-json", title: "Compliance frameworks (JSON)", path: base + "/compliance/frameworks", ext: "json", raw: false),
    ]

    public struct ExportError: Error, LocalizedError, Equatable { public let message: String; public var errorDescription: String? { message } }

    /// Checks a downloaded export before it is offered for saving: not empty, and a CSV must not be the web app's HTML page.
    public static func validate(_ data: Data, for e: Export) throws {
        guard !data.isEmpty else { throw ExportError(message: "The server returned an empty file") }
        let head = String(decoding: data.prefix(15), as: UTF8.self).lowercased()
        if e.ext != "html", head.hasPrefix("<!doctype") || head.hasPrefix("<html") {
            throw ExportError(message: "Not an API route: \(e.path)")
        }
    }
}

public extension MachinaClient {
    /// The bytes of a controller GET, for file exports (CSV and HTML are not JSON, so `call` cannot carry them).
    func controllerBytes(_ path: String) async throws -> Data {
        try await send("GET", "/api/v1/platform/controller" + path, timeout: 90)
    }
    /// An export as the bytes to save: the server's file, or the page's JSON pretty-printed.
    func exportData(_ e: AIGov.Export) async throws -> Data {
        let data = e.raw ? try await controllerBytes(e.path)
            : Data(try await call(.controllerGet(e.path)).pretty.utf8)
        try AIGov.validate(data, for: e)
        return data
    }
}

// MARK: - Cost attribution and budget

public struct TeamCost: Equatable, Sendable, Identifiable {
    public var id: String { team }
    public var team: String
    public var vmCount: Int
    public var vcpus: Int
    public var memoryGiB: Double
    public var monthlyUSD: Double
    public var exposureUSD: Double
    public var sharePct: Double
}

public struct CostAttribution: Equatable, Sendable {
    public var totalUSD: Double
    public var unattributedUSD: Double
    public var teams: [TeamCost]
    public var summary: String

    public init(json v: JSONValue) {
        totalUSD = v["total_monthly_usd"]?.double ?? 0
        unattributedUSD = v["unattributed_monthly_usd"]?.double ?? 0
        summary = v["summary"]?.text ?? ""
        teams = (v["teams"]?.array ?? []).map { t in
            TeamCost(team: t["team"]?.text ?? "", vmCount: Int(t["vm_count"]?.double ?? 0), vcpus: Int(t["vcpus"]?.double ?? 0),
                     memoryGiB: t["memory_gib"]?.double ?? 0, monthlyUSD: t["estimated_monthly_usd"]?.double ?? 0,
                     exposureUSD: t["exposure_monthly_usd"]?.double ?? 0, sharePct: t["share_pct"]?.double ?? 0)
        }
    }

    /// Chart rows, largest first; unattributed spend is a row of its own so the bars add up to the total.
    public var chartRows: [(name: String, usd: Double)] {
        var rows = teams.map { (name: $0.team.isEmpty ? "(untagged)" : $0.team, usd: $0.monthlyUSD) }
        if unattributedUSD > 0 { rows.append((name: "Unattributed", usd: unattributedUSD)) }
        return rows.sorted { $0.usd > $1.usd }
    }
}

public struct CostBudget: Equatable, Sendable {
    public var budgetUSD: Double
    public var spendUSD: Double
    public var predictedUSD: Double
    public var utilizationPct: Double
    public var status: String
    public var alerts: [String]
    public var summary: String
    public init(json v: JSONValue) {
        budgetUSD = v["monthly_budget_usd"]?.double ?? 0
        spendUSD = v["current_spend_usd"]?.double ?? 0
        predictedUSD = v["predicted_spend_usd"]?.double ?? 0
        utilizationPct = v["utilization_pct"]?.double ?? 0
        status = v["status"]?.text ?? ""
        alerts = (v["alerts"]?.array ?? []).map { "\($0["severity"]?.text ?? ""): \($0["message"]?.text ?? "")" }
        summary = v["summary"]?.text ?? ""
    }
    /// The controller derives the budget (115% of the estimate, at least 1000); it stores nothing a client could set.
    public static let setNote = "The budget is derived by the controller (115% of the current estimate, at least 1000 USD). It has no setting to change, so the app shows it read-only."
}

// MARK: - Compliance

public struct FrameworkScore: Equatable, Sendable, Identifiable {
    public var id: String { name }
    public var name: String
    public var score: Double
    public var grade: String
    public var controls: Int
    public var failed: Int
}
public struct FrameworkControl: Equatable, Sendable, Identifiable {
    public var id: String
    public var framework: String
    public var title: String
    public var passed: Bool
    public var detail: String
}
public struct ComplianceFrameworks: Equatable, Sendable {
    public var frameworks: [FrameworkScore]
    public var controls: [FrameworkControl]
    public var summary: String
    public init(json v: JSONValue) {
        summary = v["summary"]?.text ?? ""
        frameworks = (v["frameworks"]?.array ?? []).map {
            FrameworkScore(name: $0["framework"]?.text ?? "", score: $0["score"]?.double ?? 0, grade: $0["grade"]?.text ?? "",
                           controls: Int($0["control_count"]?.double ?? 0), failed: Int($0["failed_count"]?.double ?? 0))
        }
        controls = (v["controls"]?.array ?? []).map {
            FrameworkControl(id: $0["id"]?.text ?? "", framework: $0["framework"]?.text ?? "", title: $0["title"]?.text ?? "",
                             passed: $0["passed"] == .bool(true), detail: $0["detail"]?.text ?? "")
        }
    }
    public func failedControls(in framework: String) -> [FrameworkControl] { controls.filter { $0.framework == framework && !$0.passed } }
}

/// One proposal from `GET ai/compliance/remediate`. The controller has no route that applies it, so the app can only
/// show what the proposal says it would change: it never runs anything, and nothing here touches a VM.
public struct RemediationProposal: Equatable, Sendable, Identifiable {
    public var id: String
    public var framework: String
    public var control: String
    public var action: String
    public var label: String
    public var review: String
    public var risk: String
}
public enum ComplianceRemediation {
    public static let knownActions: Set<String> = ["bulk_backup", "install_guest_tools", "enable_ha", "open_security", "open_compliance"]
    public static let applyNote = "Preview only: the controller proposes these changes but has no route that applies them, so the app does not run them. Make each change from its own page."

    /// Proposals with an action the app knows; anything else (a changed or injected action name) is dropped and counted.
    public static func parse(_ v: JSONValue) -> (proposals: [RemediationProposal], rejected: Int) {
        var out: [RemediationProposal] = [], bad = 0
        for r in v["remediations"]?.array ?? [] {
            let action = r["action"]?.text ?? ""
            guard knownActions.contains(action), let id = r["id"]?.text, !id.isEmpty, id.count <= 200 else { bad += 1; continue }
            out.append(RemediationProposal(id: id, framework: r["framework"]?.text ?? "", control: r["control"]?.text ?? "", action: action,
                                           label: AIPlanText.clean(r["label"]?.text ?? ""), review: AIPlanText.clean(r["review"]?.text ?? ""),
                                           risk: r["risk"]?.text ?? ""))
        }
        return (out, bad)
    }
    /// What each action would touch, in words, for the preview list.
    public static func effect(_ action: String) -> String {
        switch action {
        case "bulk_backup": "Would back up the VMs that have no recent backup."
        case "install_guest_tools": "Would install guest tools in VMs that lack them."
        case "enable_ha": "Would enable HA on VMs without a policy."
        case "open_security": "Opens the security settings for review (no automatic change)."
        default: "Opens the compliance page for review (no automatic change)."
        }
    }
}

// MARK: - Right-sizing

public struct RightsizingRec: Equatable, Sendable, Identifiable {
    public var id: String { vmID + action }
    public var vmID: String
    public var vmName: String
    public var action: String
    public var currentMiB: Int
    public var suggestedMiB: Int
    public var savingsUSD: Double
    public var risk: String
    public var detail: String

    public init(json v: JSONValue) {
        vmID = v["vm_id"]?.text ?? ""; vmName = v["vm_name"]?.text ?? ""; action = v["action"]?.text ?? ""
        currentMiB = Int(v["current_memory_mib"]?.double ?? 0); suggestedMiB = Int(v["suggested_memory_mib"]?.double ?? 0)
        savingsUSD = v["savings_usd"]?.double ?? 0; risk = v["risk"]?.text ?? ""; detail = AIPlanText.clean(v["detail"]?.text ?? "")
    }
    public static func parse(_ v: JSONValue) -> [RightsizingRec] { (v["recommendations"]?.array ?? []).map(RightsizingRec.init(json:)) }
}

public enum RightsizingApply {
    public static let minMiB = 512
    /// Why a recommendation cannot be applied from here, or nil. Only memory shrinks on a named VM are offered.
    public static func problem(_ r: RightsizingRec) -> String? {
        if r.action != "rightsize" { return "Only memory right-sizing can be applied here (\(r.action) is advice)" }
        if UUID(uuidString: r.vmID) == nil { return "The recommendation has no valid VM id" }
        if r.vmName.isEmpty { return "The recommendation has no VM name" }
        if r.currentMiB <= 0 || r.suggestedMiB <= 0 { return "The recommendation has no memory sizes" }
        if r.suggestedMiB < minMiB { return "Suggested memory is below \(minMiB) MiB" }
        if r.suggestedMiB >= r.currentMiB { return "Suggested memory is not smaller than the current memory" }
        return nil
    }
    /// One VM only. `vcpus` (when the VM list knows it) is shown unchanged: the recommendation never alters it.
    public static func endpoint(_ r: RightsizingRec) throws -> Endpoint {
        if let p = problem(r) { throw AIGov.ExportError(message: p) }
        return Endpoint("POST", "/api/v1/vms/\(r.vmID.lowercased())/memory", body: .object(["memory_mb": .number(Double(r.suggestedMiB))]), controller: true)
    }
    public static func phrase(_ r: RightsizingRec) -> String { r.vmName }
    public static func beforeAfter(_ r: RightsizingRec, vcpus: Int?) -> [(label: String, before: String, after: String)] {
        [("vCPU", vcpus.map(String.init) ?? "unchanged", vcpus.map(String.init) ?? "unchanged"),
         ("Memory", "\(r.currentMiB) MiB", "\(r.suggestedMiB) MiB")]
    }
    public static func confirmText(_ r: RightsizingRec) -> String {
        "Resizes \(r.vmName) from \(r.currentMiB) MiB to \(r.suggestedMiB) MiB of memory through a controller task. A running VM may need a restart for a shrink to take effect, and a guest that needs more memory than this can slow down or kill processes. Only this VM changes."
    }
}

// MARK: - Digest schedule

public struct DigestSchedule: Equatable, Sendable {
    public var enabled: Bool
    public var frequency: String
    public var hourUTC: Int
    public var weekday: Int
    public var lastSentAt: String?

    public static let frequencies = ["daily", "weekly"]
    public static let weekdayNames = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]

    public init(enabled: Bool = false, frequency: String = "daily", hourUTC: Int = 8, weekday: Int = 1, lastSentAt: String? = nil) {
        self.enabled = enabled; self.frequency = frequency; self.hourUTC = hourUTC; self.weekday = weekday; self.lastSentAt = lastSentAt
    }
    public init(json v: JSONValue) {
        self.init(enabled: v["enabled"] == .bool(true), frequency: v["frequency"]?.text ?? "daily", hourUTC: Int(v["hour_utc"]?.double ?? 8),
                  weekday: Int(v["weekday"]?.double ?? 1), lastSentAt: { let t = v["last_sent_at"]?.text ?? ""; return v["last_sent_at"] == .null || t.isEmpty ? nil : t }())
    }
    /// Mirrors the controller's `validate`.
    public var problem: String? {
        if !Self.frequencies.contains(frequency) { return "frequency must be daily or weekly" }
        if !(0...23).contains(hourUTC) { return "hour must be 0 to 23 (UTC)" }
        if !(1...7).contains(weekday) { return "weekday must be 1 (Monday) to 7 (Sunday)" }
        return nil
    }
    public func putEndpoint() throws -> Endpoint {
        if let p = problem { throw AIGov.ExportError(message: p) }
        return Endpoint("PUT", AIGov.base + "/digest/schedule", body: .object([
            "enabled": .bool(enabled), "frequency": .string(frequency), "hour_utc": .number(Double(hourUTC)), "weekday": .number(Double(weekday))]), controller: true)
    }
    public var confirmText: String {
        enabled ? "Turns the scheduled digest on: \(frequency), \(String(format: "%02d", hourUTC)):00 UTC\(frequency == "weekly" ? ", " + Self.weekdayNames[max(0, min(6, weekday - 1))] : ""). It is sent to every webhook and notification channel subscribed to zyra.digest."
                : "Turns the scheduled digest off. Nothing is sent on a schedule until it is switched on again."
    }
    public static let sendPhrase = "SEND DIGEST"
    public static let sendText = "Sends the fleet digest now, outside the schedule, to every webhook and notification channel subscribed to zyra.digest (zyra.* or *). It leaves the controller and cannot be recalled."
}

// MARK: - Intent environment

/// A plan generated from a sentence. It is data from a generator the app does not control: every field is bounded,
/// the text is stripped of control characters, and a plan that fails the checks is never offered for execution.
public struct EnvironmentPlan: Equatable, Sendable {
    public var label: String, review: String, type: String
    public var vmCount: Int, vcpusPerVM: Int, memoryGiBPerVM: Int, storageGiB: Int
    public var monthlyUSD: Double
    public var gpu: Bool
    public var steps: [String]
    public init(json v: JSONValue) {
        label = AIPlanText.clean(v["label"]?.text ?? ""); review = AIPlanText.clean(v["review"]?.text ?? ""); type = v["environment_type"]?.text ?? ""
        vmCount = Int(v["vm_count"]?.double ?? 0); vcpusPerVM = Int(v["vcpus_per_vm"]?.double ?? 0)
        memoryGiBPerVM = Int(v["memory_gib_per_vm"]?.double ?? 0); storageGiB = Int(v["storage_gib"]?.double ?? 0)
        monthlyUSD = v["estimated_monthly_usd"]?.double ?? 0; gpu = v["gpu_required"] == .bool(true)
        steps = (v["build_steps"]?.array ?? []).map { AIPlanText.clean($0.text) }
    }
}

public enum EnvironmentIntent {
    public static let planEndpointPath = AIGov.base + "/intent/environment"
    public static let executeEndpointPath = AIGov.base + "/intent/environment/execute"
    public static let maxQuery = 500
    public static let maxVMsPerRun = 5      // the server default; the app never asks for more
    static let typeOK = try! NSRegularExpression(pattern: "^[a-z0-9][a-z0-9-]{0,31}$")
    static let vmNameOK = try! NSRegularExpression(pattern: "^[a-z0-9][a-z0-9-]{0,40}-dev-[0-9]{2}$")

    public static func queryProblem(_ q: String) -> String? {
        let t = q.trimmingCharacters(in: .whitespacesAndNewlines)
        if t.isEmpty { return "Describe the environment in a sentence" }
        if t.count > maxQuery { return "Keep it under \(maxQuery) characters" }
        if t.unicodeScalars.contains(where: { $0.value < 0x20 && $0 != "\n" || $0.value == 0x7f }) { return "Contains characters that are not allowed" }
        return nil
    }
    public static func planEndpoint(query: String) throws -> Endpoint {
        if let p = queryProblem(query) { throw AIGov.ExportError(message: p) }
        return Endpoint("POST", planEndpointPath, body: .object(["query": .string(query.trimmingCharacters(in: .whitespacesAndNewlines))]), controller: true)
    }
    /// The run body. `dryRun` true asks the controller to place VMs without creating anything.
    public static func executeEndpoint(query: String, dryRun: Bool, maxVMs: Int) throws -> Endpoint {
        if let p = queryProblem(query) { throw AIGov.ExportError(message: p) }
        return Endpoint("POST", executeEndpointPath, body: .object([
            "query": .string(query.trimmingCharacters(in: .whitespacesAndNewlines)), "dry_run": .bool(dryRun),
            "max_vms": .number(Double(max(1, min(maxVMs, maxVMsPerRun))))]), controller: true, timeout: 120)
    }

    /// All the reasons a plan is refused (empty = acceptable).
    public static func problems(_ p: EnvironmentPlan) -> [String] {
        var out: [String] = []
        func range(_ n: Int, _ r: ClosedRange<Int>, _ what: String) { if !r.contains(n) { out.append("\(what) \(n) is outside \(r.lowerBound) to \(r.upperBound)") } }
        range(p.vmCount, 1...100, "VM count"); range(p.vcpusPerVM, 1...128, "vCPUs per VM")
        range(p.memoryGiBPerVM, 1...2048, "Memory per VM (GiB)"); range(p.storageGiB, 0...100_000, "Storage (GiB)")
        if p.gpu { out.append("GPU environments are not run from here (the controller refuses them)") }
        if !p.monthlyUSD.isFinite || p.monthlyUSD < 0 { out.append("Estimated cost is not a valid number") }
        if typeOK.firstMatch(in: p.type, range: NSRange(p.type.startIndex..., in: p.type)) == nil { out.append("Environment type '\(p.type)' is not a plain name") }
        if p.steps.count > 40 { out.append("More than 40 steps") }
        if p.steps.contains(where: { $0.isEmpty || $0.count > 400 }) { out.append("A step is empty or longer than 400 characters") }
        return out
    }
    /// The parsed plan, or the list of reasons it is refused.
    public static func validated(_ json: JSONValue) -> Result<EnvironmentPlan, PlanRefused> {
        guard json.object != nil, json["vm_count"] != nil else { return .failure(PlanRefused(reasons: ["The answer is not a plan"])) }
        let p = EnvironmentPlan(json: json)
        let r = problems(p)
        return r.isEmpty ? .success(p) : .failure(PlanRefused(reasons: r))
    }
    public struct PlanRefused: Error, Equatable, LocalizedError { public let reasons: [String]; public var errorDescription: String? { reasons.joined(separator: "; ") } }

    /// Dry-run placements, kept only when each VM name has the shape the controller generates (`<type>-dev-NN`).
    public static func placements(_ json: JSONValue) -> [(name: String, host: String)] {
        (json["vm_tasks"]?.array ?? []).compactMap { t in
            let n = t["name"]?.text ?? "", h = AIPlanText.clean(t["host"]?.text ?? "")
            guard vmNameOK.firstMatch(in: n, range: NSRange(n.startIndex..., in: n)) != nil else { return nil }
            return (n, h)
        }
    }
    public static func phrase(_ p: EnvironmentPlan, vms: Int) -> String { "CREATE \(vms) VMS" }
    public static func confirmText(_ p: EnvironmentPlan, vms: Int) -> String {
        "Creates \(vms) VM(s) for the \(p.type) environment (\(p.vcpusPerVM) vCPU, \(p.memoryGiBPerVM) GiB, 80 GiB root each), placed by the controller. They start costing about \(String(format: "%.0f", p.monthlyUSD)) USD a month for the full plan. The steps listed are the plan's description; only the VM creation above is run."
    }
}

public enum AIPlanText {
    /// Generated text shown to people: control characters removed, length capped.
    public static func clean(_ s: String, limit: Int = 600) -> String {
        let t = String(String.UnicodeScalarView(s.unicodeScalars.filter { ($0.value >= 0x20 && $0.value != 0x7f) || $0 == "\n" }))
        return String(t.prefix(limit))
    }
}
