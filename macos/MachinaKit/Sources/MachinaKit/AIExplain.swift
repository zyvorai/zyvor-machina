import Foundation

/// Contextual "Explain with AI" and the Knowledge tools (controller `api/ai.rs`, operator role).
///
/// Safety rules, enforced here and covered by tests:
/// - a request carries only ids and fields the user selected; secret-looking keys are dropped, long strings cut;
/// - everything the server answers is untrusted text: control and bidi characters are stripped, it is shown as
///   plain text (never Markdown or links), and nothing in it is executed; remediation steps and commands are read-only text;
/// - nothing is cached to disk; Markdown export only happens when the user saves a file, and escapes server text.

// MARK: Text hygiene

public enum AIText {
    public static let maxLength = 20_000

    /// Strips control characters (keeping newline and tab), bidirectional overrides and zero-width characters, normalises
    /// line endings, and caps the length. The result is still untrusted: show it with `Text(verbatim:)`.
    public static func clean(_ s: String, limit: Int = maxLength) -> String {
        var out = String.UnicodeScalarView()
        var count = 0
        let scalars = Array(s.replacingOccurrences(of: "\r\n", with: "\n").replacingOccurrences(of: "\r", with: "\n").unicodeScalars)
        for u in scalars {
            let v = u.value
            let isControl = v < 0x20 && v != 0x0A && v != 0x09 || v == 0x7F || (0x80...0x9F).contains(v)
            let isBidi = (0x202A...0x202E).contains(v) || (0x2066...0x2069).contains(v) || v == 0x200E || v == 0x200F || v == 0x061C
            let isInvisible = (0x200B...0x200D).contains(v) || v == 0x2060 || v == 0xFEFF
            if isControl || isBidi || isInvisible { continue }
            if count >= limit { out.append(contentsOf: "…".unicodeScalars); break }
            out.append(u); count += 1
        }
        return String(out).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Escapes Markdown so server text cannot become a link, image, heading or HTML in an exported file.
    public static func markdownEscaped(_ s: String) -> String {
        var out = ""
        for ch in clean(s) {
            if "\\`*_{}[]()<>#+!|~".contains(ch) { out.append("\\") }
            out.append(ch)
        }
        return out
    }

    /// A fenced block whose fence is longer than any backtick run inside, so the content cannot close it.
    public static func fenced(_ s: String) -> String {
        let body = clean(s)
        var longest = 0, run = 0
        for ch in body { if ch == "`" { run += 1; longest = max(longest, run) } else { run = 0 } }
        let fence = String(repeating: "`", count: max(3, longest + 1))
        return "\(fence)\n\(body)\n\(fence)"
    }
}

/// Drops secret-looking keys from what is sent as context and cuts long strings.
public enum AIScrub {
    static let markers = ["password", "passwd", "secret", "token", "api_key", "apikey", "cookie", "authorization", "credential", "private", "session", "bearer"]
    public static func isSecretKey(_ key: String) -> Bool { let k = key.lowercased(); return markers.contains { k.contains($0) } }

    public static func scrub(_ v: JSONValue, depth: Int = 0) -> JSONValue {
        switch v {
        case .object(let o):
            if depth >= 4 { return .null }
            var out: [String: JSONValue] = [:]
            for (k, x) in o where !isSecretKey(k) { out[k] = scrub(x, depth: depth + 1) }
            return .object(out)
        case .array(let a): return depth >= 4 ? .null : .array(a.prefix(50).map { scrub($0, depth: depth + 1) })
        case .string(let s): return .string(s.count > 500 ? String(s.prefix(500)) : s)
        default: return v
        }
    }
}

// MARK: Requests

/// What the user asked to have explained. Each case maps to one endpoint and carries only selected ids and fields.
public enum AIExplainRequest: Equatable, Sendable {
    case vm(id: String?, name: String)
    case alert(id: String, rule: String, severity: String, message: String)
    case failedTask(id: String, operation: String, status: String, message: String?)
    case securityEvent(fields: JSONValue, hostID: String?)
    case firewall(targetID: String, question: String?)
    case network(vmA: String, vmB: String, port: Int?)

    public var title: String {
        switch self {
        case .vm(_, let n): return "Why is \(AIText.clean(n, limit: 80)) unhealthy?"
        case .alert(_, let r, _, _): return "Alert: \(AIText.clean(r, limit: 80))"
        case .failedTask(_, let op, _, _): return "Failed task: \(AIText.clean(op, limit: 80))"
        case .securityEvent: return "Security event"
        case .firewall: return "Firewall target"
        case .network(let a, let b, _): return "\(AIText.clean(a, limit: 60)) to \(AIText.clean(b, limit: 60))"
        }
    }

    /// Endpoint and body of the main call.
    public var endpoint: Endpoint {
        switch self {
        case .vm(let id, let name):
            var ref: [String: JSONValue] = ["vm_name": .string(name)]
            if let id { ref["vm_id"] = .string(id) }
            return Self.post("/api/v1/ai/explain", ["screen": .string("vm_detail"), "object_ref": .object(ref)])
        case .alert(let id, let rule, let sev, let msg):
            return Self.post("/api/v1/ai/explain", ["screen": .string("notification"),
                "object_ref": .object(["alert_id": .string(id), "rule_name": .string(rule), "severity": .string(sev), "message": .string(Self.cut(msg))])])
        case .failedTask(let id, let op, let status, let msg):
            var ref: [String: JSONValue] = ["task_id": .string(id), "operation": .string(op), "status": .string(status)]
            if let msg { ref["message"] = .string(Self.cut(msg)) }
            return Self.post("/api/v1/ai/explain", ["screen": .string("failed_task"), "object_ref": .object(ref)])
        case .securityEvent(let fields, let host):
            var b: [String: JSONValue] = ["event": AIScrub.scrub(fields)]
            if let host { b["host_id"] = .string(host) }
            return Self.post("/api/v1/ai/security/explain-event", b)
        case .firewall(let target, let q):
            var b: [String: JSONValue] = ["target_id": .string(target)]
            if let q, !q.isEmpty { b["question"] = .string(Self.cut(q)) }
            return Self.post("/api/v1/ai/firewall/explain", b)
        case .network(let a, let b, let port):
            var body: [String: JSONValue] = ["vm_a": .string(a), "vm_b": .string(b)]
            if let port { body["port"] = .number(Double(port)) }
            return Self.post("/api/v1/ai/network/explain", body)
        }
    }

    /// Alerts and failed tasks also ask for a runbook so remediation steps can be listed. Optional: a failure is ignored.
    public var runbookEndpoint: Endpoint? {
        switch self {
        case .alert(let id, let rule, let sev, let msg):
            return Self.post("/api/v1/ai/runbook", ["incident": .string(Self.cut("\(rule): \(msg)")),
                "context": .object(["alert_id": .string(id), "severity": .string(sev)])])
        case .failedTask(let id, let op, _, let msg):
            return Self.post("/api/v1/ai/runbook", ["incident": .string(Self.cut("Task \(op) failed" + (msg.map { ": \($0)" } ?? ""))),
                "context": .object(["task_id": .string(id)])])
        default: return nil
        }
    }

    static func cut(_ s: String) -> String { s.count > 1000 ? String(s.prefix(1000)) : s }
    static func post(_ path: String, _ body: [String: JSONValue]) -> Endpoint {
        Endpoint("POST", path, body: .object(body), controller: true, timeout: 60)
    }

    /// Which request a resource page row can produce (nil: the page has no explain action).
    public static func forRow(specID: String, row: ResourceRow) -> AIExplainRequest? {
        switch specID {
        case "fw-targets": return .firewall(targetID: row.id, question: nil)
        case "fw-anomalies", "bpf-anomalies", "bpf-events", "bpf-flows", "bpf-processes", "bpf-dns":
            guard row.values.object != nil else { return nil }
            return .securityEvent(fields: row.values, hostID: nil)
        default: return nil
        }
    }
}

// MARK: Results

public struct AIFact: Equatable, Sendable, Identifiable {
    public var label: String
    public var value: String
    public var id: String { label }
}

/// A parsed answer. Every string is cleaned; steps and commands are text for the user to read.
public struct AIExplanation: Equatable, Sendable {
    public var title: String
    public var text: String
    public var facts: [AIFact] = []
    public var steps: [String] = []
    public var commands: [String] = []

    public init(title: String, text: String, facts: [AIFact] = [], steps: [String] = [], commands: [String] = []) {
        self.title = title; self.text = text; self.facts = facts; self.steps = steps; self.commands = commands
    }

    public static func parse(_ req: AIExplainRequest, _ v: JSONValue) -> AIExplanation {
        var e = AIExplanation(title: req.title, text: "")
        func str(_ k: String) -> String { v[k].flatMap { $0.isScalar && $0 != .null ? AIText.clean($0.text) : nil } ?? "" }
        func list(_ k: String, in j: JSONValue? = nil) -> [String] {
            ((j ?? v)[k]?.array ?? []).compactMap { $0.isScalar && $0 != .null ? AIText.clean($0.text) : nil }.filter { !$0.isEmpty }
        }
        switch req {
        case .securityEvent:
            e.text = str("explanation")
            e.facts = [("Risk", str("risk")), ("Recommendation", str("recommendation"))].filter { !$0.1.isEmpty }.map { AIFact(label: $0.0, value: $0.1) }
        case .firewall:
            e.text = str("summary")
            e.facts = [("Risk", str("risk")), ("Recommendation", str("recommendation"))].filter { !$0.1.isEmpty }.map { AIFact(label: $0.0, value: $0.1) }
            e.steps = list("evidence")
        case .network:
            e.text = str("explanation")
            if let ok = v["can_reach"], case .bool(let b) = ok { e.facts.append(AIFact(label: "Reachable", value: b ? "yes" : "no")) }
            if !str("remediation").isEmpty { e.facts.append(AIFact(label: "Remediation", value: str("remediation"))) }
            e.steps = list("hops")
        default:
            e.text = str("explanation")
        }
        return e
    }

    /// Merges a runbook answer (`steps`, `commands`) into this explanation.
    public mutating func addRunbook(_ v: JSONValue) {
        steps += (v["steps"]?.array ?? []).compactMap { $0.isScalar && $0 != .null ? AIText.clean($0.text) : nil }.filter { !$0.isEmpty }
        commands += (v["commands"]?.array ?? []).compactMap { $0.isScalar && $0 != .null ? AIText.clean($0.text) : nil }.filter { !$0.isEmpty }
    }

    public var isEmpty: Bool { text.isEmpty && facts.isEmpty && steps.isEmpty && commands.isEmpty }

    public func markdown() -> String {
        var out = "# \(AIText.markdownEscaped(title))\n\n"
        if !text.isEmpty { out += AIText.markdownEscaped(text) + "\n\n" }
        for f in facts { out += "**\(AIText.markdownEscaped(f.label)):** \(AIText.markdownEscaped(f.value))\n\n" }
        if !steps.isEmpty { out += "## Steps\n\n" + steps.enumerated().map { "\($0.offset + 1). \(AIText.markdownEscaped($0.element))" }.joined(separator: "\n") + "\n\n" }
        if !commands.isEmpty { out += "## Commands (review before running)\n\n" + AIText.fenced(commands.joined(separator: "\n")) + "\n" }
        return out
    }
}

// MARK: Knowledge

public struct KnowledgeHit: Equatable, Sendable, Identifiable {
    public var kind: String, title: String, snippet: String, score: Double
    public var id: String { kind + "|" + title + "|" + snippet.prefix(30) }
}

public struct KnowledgeHypothesis: Equatable, Sendable, Identifiable {
    public var title: String, evidence: String, action: String
    /// 0...1
    public var confidence: Double
    public var id: String { title + evidence.prefix(20) }
}

public struct KnowledgeDiagnosis: Equatable, Sendable {
    public var query: String
    public var summary: String
    public var extra: [AIFact]
    public var hypotheses: [KnowledgeHypothesis]

    public func markdown() -> String {
        var out = "# Diagnosis\n\n**Question:** \(AIText.markdownEscaped(query))\n\n"
        if !summary.isEmpty { out += AIText.markdownEscaped(summary) + "\n\n" }
        for f in extra { out += "**\(AIText.markdownEscaped(f.label)):** \(AIText.markdownEscaped(f.value))\n\n" }
        for (i, h) in hypotheses.enumerated() {
            out += "## \(i + 1). \(AIText.markdownEscaped(h.title)) (\(Int((h.confidence * 100).rounded()))%)\n\n"
            if !h.evidence.isEmpty { out += "Evidence: \(AIText.markdownEscaped(h.evidence))\n\n" }
            if !h.action.isEmpty { out += "Suggested action: \(AIText.markdownEscaped(h.action))\n\n" }
        }
        return out
    }
}

public struct KnowledgeRunbook: Equatable, Sendable {
    public var query: String, title: String, summary: String
    public var steps: [String], commands: [String]

    public func markdown() -> String {
        var out = "# \(AIText.markdownEscaped(title.isEmpty ? "Runbook" : title))\n\n**Question:** \(AIText.markdownEscaped(query))\n\n"
        if !summary.isEmpty { out += AIText.markdownEscaped(summary) + "\n\n" }
        if !steps.isEmpty { out += "## Steps\n\n" + steps.enumerated().map { "\($0.offset + 1). \(AIText.markdownEscaped($0.element))" }.joined(separator: "\n") + "\n\n" }
        if !commands.isEmpty { out += "## Commands (review before running)\n\n" + AIText.fenced(commands.joined(separator: "\n")) + "\n" }
        return out
    }
    /// Plain text for the clipboard.
    public func plain() -> String {
        var out = title.isEmpty ? "Runbook" : title
        if !summary.isEmpty { out += "\n\n" + summary }
        out += steps.enumerated().map { "\n\($0.offset + 1). \($0.element)" }.joined()
        if !commands.isEmpty { out += "\n\nCommands:\n" + commands.joined(separator: "\n") }
        return out
    }
}

// MARK: Client

public enum AIFailure: Error, Equatable, LocalizedError, Sendable {
    case unavailable(String), forbidden, emptyQuery, failed(String)
    public var errorDescription: String? {
        switch self {
        case .unavailable(let why): return "AI unavailable. \(why)"
        case .forbidden: return "AI explanations need the operator role."
        case .emptyQuery: return "Type a question first."
        case .failed(let m): return m
        }
    }
    /// Maps a transport or server error to a user-facing state. The server's message is untrusted and cleaned.
    public static func map(_ e: Error) -> AIFailure {
        if let f = e as? AIFailure { return f }
        if let m = e as? MachinaError {
            switch m {
            case .http(let code, let body):
                if code == 403 { return .forbidden }
                if [404, 405, 501, 502, 503, 504].contains(code) { return .unavailable("The controller or its AI provider did not answer (HTTP \(code)). Nothing was changed.") }
                return .failed("HTTP \(code): \(AIText.clean(body, limit: 200))")
            case .timedOut: return .unavailable("The AI provider did not answer in time. Try again.")
            case .unauthenticated: return .failed("Sign in required.")
            case .badURL: return .failed("Invalid server URL.")
            }
        }
        if e is URLError { return .unavailable("The server could not be reached.") }
        return .failed(AIText.clean(e.localizedDescription, limit: 200))
    }
}

public extension MachinaClient {
    func aiExplain(_ req: AIExplainRequest) async throws -> AIExplanation {
        do {
            let v = try await call(req.endpoint)
            var e = AIExplanation.parse(req, v)
            if let rb = req.runbookEndpoint, let r = try? await call(rb) { e.addRunbook(r) }
            if e.isEmpty { throw AIFailure.unavailable("The server returned no explanation.") }
            return e
        } catch { throw AIFailure.map(error) }
    }

    func knowledgeSearch(_ q: String) async throws -> [KnowledgeHit] {
        let query = try AIQuery.validated(q)
        do {
            let v = try await call(AIExplainRequest.post("/api/v1/ai/knowledge/search", ["query": .string(query)]))
            return (v["hits"]?.array ?? []).compactMap { h in
                guard let t = h["title"] else { return nil }
                return KnowledgeHit(kind: AIText.clean(h["kind"]?.text ?? "", limit: 40), title: AIText.clean(t.text, limit: 300),
                                    snippet: AIText.clean(h["snippet"]?.text ?? "", limit: 2000), score: h["score"]?.double ?? 0)
            }
        } catch { throw AIFailure.map(error) }
    }

    /// `fleet` asks `/ai/fleet/diagnose` (adds the Zyra and Linux summaries); otherwise `/ai/knowledge/diagnose`.
    func knowledgeDiagnose(_ q: String, fleet: Bool = false) async throws -> KnowledgeDiagnosis {
        let query = try AIQuery.validated(q)
        do {
            let v = try await call(AIExplainRequest.post(fleet ? "/api/v1/ai/fleet/diagnose" : "/api/v1/ai/knowledge/diagnose", ["query": .string(query)]))
            let hyps = (v["hypotheses"]?.array ?? []).compactMap { h -> KnowledgeHypothesis? in
                guard let t = h["title"] else { return nil }
                var c = h["confidence"]?.double ?? 0
                if c > 1 { c /= 100 }
                return KnowledgeHypothesis(title: AIText.clean(t.text, limit: 300), evidence: AIText.clean(h["evidence"]?.text ?? "", limit: 4000),
                                           action: AIText.clean(h["action"]?.text ?? "", limit: 2000), confidence: min(max(c, 0), 1))
            }
            let extra = [("Zyra status", "zyra_status"), ("Linux", "linux_summary")].compactMap { (l, k) -> AIFact? in
                guard let s = v[k]?.text, s != "–", !s.isEmpty else { return nil }
                return AIFact(label: l, value: AIText.clean(s))
            }
            return KnowledgeDiagnosis(query: query, summary: AIText.clean(v["summary"]?.text ?? ""), extra: extra, hypotheses: hyps)
        } catch { throw AIFailure.map(error) }
    }

    func knowledgeRunbook(_ q: String) async throws -> KnowledgeRunbook {
        let query = try AIQuery.validated(q)
        do {
            let v = try await call(AIExplainRequest.post("/api/v1/ai/knowledge/runbook", ["query": .string(query)]))
            func list(_ k: String) -> [String] { (v[k]?.array ?? []).compactMap { $0.isScalar && $0 != .null ? AIText.clean($0.text) : nil }.filter { !$0.isEmpty } }
            let rb = KnowledgeRunbook(query: query, title: AIText.clean(v["runbook_title"]?.text ?? "", limit: 300),
                                      summary: AIText.clean(v["summary"]?.text ?? v["diagnosis_summary"]?.text ?? ""), steps: list("steps"), commands: list("commands"))
            if rb.steps.isEmpty && rb.commands.isEmpty && rb.summary.isEmpty { throw AIFailure.unavailable("The server returned no runbook.") }
            return rb
        } catch { throw AIFailure.map(error) }
    }
}

public enum AIQuery {
    public static let maxLength = 2000
    public static func validated(_ s: String) throws -> String {
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        if t.isEmpty { throw AIFailure.emptyQuery }
        return String(t.prefix(maxLength))
    }
}
