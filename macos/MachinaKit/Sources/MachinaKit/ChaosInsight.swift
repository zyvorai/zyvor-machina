import Foundation

/// One phase of a chaos run report (controller `engine/chaos.rs` `PhaseReport`): the baseline, each step, the recovery.
public struct ChaosPhase: Equatable, Sendable, Identifiable {
    public var id: Int
    public var name: String
    public var kind: String
    public var startedS: Double
    public var endedS: Double
    public var samples: Int
    public var ok: Int
    public var successPct: Double?
    public var p50ms: Int?
    public var p95ms: Int?
    public var notes: [String]
    public var recoveredS: Double?
    public var error: String?

    public init(index: Int, json v: JSONValue) {
        id = index
        name = v["name"]?.text ?? "Phase \(index + 1)"
        kind = v["kind"]?.text ?? ""
        startedS = v["started_s"]?.double ?? 0
        endedS = v["ended_s"]?.double ?? 0
        samples = v["samples"]?.double.map { Int($0) } ?? 0
        ok = v["ok"]?.double.map { Int($0) } ?? 0
        successPct = v["success_pct"]?.double
        p50ms = v["p50_ms"]?.double.map { Int($0) }
        p95ms = v["p95_ms"]?.double.map { Int($0) }
        notes = (v["notes"]?.array ?? []).map(\.text)
        recoveredS = v["recovered_s"]?.double
        let e = v["error"]?.text ?? ""
        error = (v["error"] == nil || v["error"] == .null || e.isEmpty) ? nil : e
    }

    public var seconds: Double { max(0, endedS - startedS) }
    /// Pass when the phase had no error and probe success stayed at or above the abort threshold
    /// (a phase without probe samples cannot fail on probes). 50 is the controller's default threshold.
    public func passed(minSuccessPct: Double = 50) -> Bool {
        if error != nil { return false }
        guard let p = successPct, samples > 0 else { return true }
        return p >= minSuccessPct
    }
}

/// `GET /api/v1/chaos/runs/{id}` -> `{run, live}`: the run row, its report once finished, and live phases while it runs.
public struct ChaosRunDetail: Equatable, Sendable {
    public var id: String
    public var experimentID: String
    public var status: String
    public var startedBy: String
    public var startedAt: String
    public var finishedAt: String?
    public var abortReason: String
    public var verdict: String
    public var findings: [String]
    public var probes: [String]
    public var targets: [String]
    public var durationS: Double?
    public var phases: [ChaosPhase]
    /// Index of the phase running now (live runs only).
    public var livePhase: Int?

    public init(json v: JSONValue) {
        let run = v["run"] ?? v
        id = run["id"]?.text ?? ""
        experimentID = run["experiment_id"]?.text ?? ""
        status = run["status"]?.text ?? ""
        startedBy = run["started_by"]?.text ?? ""
        startedAt = run["started_at"]?.text ?? ""
        let f = run["finished_at"]?.text ?? ""
        finishedAt = (run["finished_at"] == nil || run["finished_at"] == .null || f.isEmpty) ? nil : f
        abortReason = run["abort_reason"]?.text ?? ""
        let report = run["report"] ?? .null
        verdict = report["verdict"]?.text ?? ""
        findings = (report["findings"]?.array ?? []).map(\.text)
        probes = (report["probes"]?.array ?? []).map(\.text)
        targets = (report["targets"]?.array ?? []).map(\.text)
        durationS = report["duration_s"]?.double
        let live = v["live"]
        let finished = report["phases"]?.array ?? []
        let source = finished.isEmpty ? (live?["phases"]?.array ?? []) : finished
        phases = source.enumerated().map { ChaosPhase(index: $0.offset, json: $0.element) }
        livePhase = live?["phase"]?.double.map { Int($0) }
    }

    /// Only a running run can be aborted.
    public var canAbort: Bool { status == "running" }
    public var passedPhases: Int { phases.filter { $0.passed() }.count }
    public var failedPhases: Int { phases.count - passedPhases }
}

public extension ChaosAPI {
    static func getRun(id: String) -> Endpoint { .controllerGet("/api/v1/chaos/runs/\(MachinaClient.seg(id))") }
}

/// Duplicate and JSON import/export of experiments. An imported or duplicated experiment is only ever a draft: it
/// targets nothing (VM ids belong to one controller), is validated like any draft, and is saved, never run.
public enum ChaosPorting {
    public struct ImportError: LocalizedError, Equatable {
        public let message: String
        public var errorDescription: String? { message }
    }
    public static let maxBytes = 256 * 1024
    public static let format = "machina-chaos-experiment"

    /// A copy named "<name> (copy)" with no targets, to be retargeted deliberately.
    public static func duplicate(_ d: ChaosDraft) -> ChaosDraft {
        var c = d
        let base = d.name.trimmingCharacters(in: .whitespaces)
        c.name = String("\(base) (copy)".prefix(64))
        c.targets = []
        return c
    }

    /// Experiment as portable JSON. Targets are left out on purpose: they are VM ids of this controller.
    public static func export(_ d: ChaosDraft) throws -> Data {
        let body: JSONValue = .object([
            "format": .string(format), "version": .number(1),
            "name": .string(d.name), "description": .string(d.description),
            "spec": .object([
                "steps": .array(d.steps.map(\.json)), "probes": .array(d.probes),
                "abort": .object(["min_success_pct": .number(d.minSuccessPct), "window_secs": .number(Double(d.windowSecs))]),
                "baseline_secs": .number(Double(d.baselineSecs)), "recovery_secs": .number(Double(d.recoverySecs)),
            ]),
        ])
        let enc = JSONEncoder(); enc.outputFormatting = [.prettyPrinted, .sortedKeys]
        return try enc.encode(body)
    }

    /// Parses and validates imported JSON. Accepts this app's export and the controller's own experiment JSON.
    /// Any target list in the file is dropped. Never runs anything.
    public static func importDraft(_ data: Data) throws -> ChaosDraft {
        if data.count > maxBytes { throw ImportError(message: "That file is too large for an experiment.") }
        guard let v = try? JSONDecoder().decode(JSONValue.self, from: data), v.object != nil else {
            throw ImportError(message: "That file is not a JSON object.")
        }
        guard let spec = v["spec"], spec.object != nil else { throw ImportError(message: "No experiment spec found in that file.") }
        let rawSteps = spec["steps"]?.array ?? []
        var d = ChaosDraft(row: v)
        if d.steps.count != rawSteps.count {
            throw ImportError(message: "\(rawSteps.count - d.steps.count) step(s) have an unknown kind and cannot be imported.")
        }
        d.targets = []
        // Same rules as any draft; the placeholder target only satisfies the "pick a target" rule that import deliberately skips.
        var probe = d; probe.targets = ["00000000-0000-0000-0000-000000000000"]
        if let e = probe.validate() { throw ImportError(message: e) }
        return d
    }
}
