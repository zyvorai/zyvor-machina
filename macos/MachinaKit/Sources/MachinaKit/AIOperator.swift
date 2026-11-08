import Foundation

// The conversational AI operator (Zeus): pure logic. Everything the model says is untrusted data.
//
// - A plan is only ever *displayed*. The app never runs anything on its own; each action is approved one at a
//   time through the same confirmation and role gate as a manual action.
// - Before an action is even offered it must name an allowlisted action type, fit the user's role and (when the
//   answer names an endpoint or verb) match the endpoint allowlist. Anything else is dropped and counted.
// - The request carries only what the user typed. No credentials, cookies or keychain data, and no prompt is logged.

// MARK: Allowlist

public struct AIActionPolicy: Hashable, Sendable {
    public var type: String
    public var adminOnly: Bool
    public var destructive: Bool
    /// Typed confirmation the user must enter (in addition to the confirmation dialog) for the riskiest actions.
    public var typedPhrase: String?
    public init(_ type: String, adminOnly: Bool = false, destructive: Bool = false, typedPhrase: String? = nil) {
        self.type = type; self.adminOnly = adminOnly; self.destructive = destructive; self.typedPhrase = typedPhrase
    }
}

public enum AIAllowlist {
    /// Action types the controller's `approve_and_execute` knows how to run. Everything else is refused.
    public static let policies: [String: AIActionPolicy] = Dictionary(uniqueKeysWithValues: [
        AIActionPolicy("start_vm"),
        AIActionPolicy("stop_vm", destructive: true),
        AIActionPolicy("create_backup"),
        AIActionPolicy("enable_ha"),
        AIActionPolicy("install_guest_tools"),
        AIActionPolicy("create_volume"),
        AIActionPolicy("create_security_group_allow", destructive: true),
        AIActionPolicy("guest.sync_time"),
        AIActionPolicy("guest.fstrim"),
        AIActionPolicy("sync_hosts"),
        AIActionPolicy("bulk_backup", destructive: true),
        AIActionPolicy("bulk_ha", destructive: true),
        AIActionPolicy("create_environment", destructive: true),
        AIActionPolicy("adopt_vm", adminOnly: true, destructive: true),
        AIActionPolicy("remove_stale_vm", adminOnly: true, destructive: true, typedPhrase: "REMOVE"),
    ].map { ($0.type, $0) })

    public static func policy(for type: String) -> AIActionPolicy? { policies[type] }

    /// The only endpoints this feature calls. A path or verb named by a model answer must match one of these.
    public static func permits(method: String, path: String) -> Bool {
        let m = method.uppercased()
        let prefix = "/api/v1/platform/controller"
        let p = path.hasPrefix(prefix) ? String(path.dropFirst(prefix.count)) : path
        if m == "POST", p == "/api/v1/ai/nl-ops" || p == "/api/v1/ai/zyra/chat" { return true }
        if m == "GET", p == "/api/v1/ai/providers" { return true }
        let parts = p.split(separator: "/", omittingEmptySubsequences: false).map(String.init)   // "", api, v1, ai, actions, <id>, verb
        if m == "POST", parts.count == 7, Array(parts[0...4]) == ["", "api", "v1", "ai", "actions"],
           UUID(uuidString: parts[5]) != nil, parts[6] == "execute" || parts[6] == "reject" { return true }
        return false
    }

    /// Whether this role may be offered the action at all (readonly sees nothing; admin-only types need admin).
    public static func allows(_ policy: AIActionPolicy, role: String?) -> Bool {
        switch role { case "admin": true; case "operator": !policy.adminOnly; default: false }
    }
}

// MARK: Plan model

public struct AIProposedAction: Identifiable, Hashable, Sendable {
    public var id: Int                       // index in the plan
    public var label: String
    public var actionType: String
    public var review: String
    public var risk: String
    public var policy: AIActionPolicy
    /// The queued server-side action this step maps to. Nil means it can be read but not approved from here.
    public var actionId: UUID?
    public var isExecutable: Bool { actionId != nil }
}

public struct AIDroppedAction: Hashable, Sendable {
    public var label: String
    public var reason: String
}

public struct AIPlan: Hashable, Sendable {
    public var intent: String
    public var summary: String
    public var reply: String
    public var riskScore: Int
    public var actions: [AIProposedAction]
    public var dropped: [AIDroppedAction]
    public static let empty = AIPlan(intent: "", summary: "", reply: "", riskScore: 0, actions: [], dropped: [])
}

public enum AIPlanParser {
    public static let maxText = 600
    public static let maxActions = 12

    /// Strips control characters and caps the length; model text is shown as plain text only.
    public static func clean(_ s: String, limit: Int = maxText) -> String {
        let bad = CharacterSet.controlCharacters.subtracting(.newlines)
        let t = String(String.UnicodeScalarView(s.unicodeScalars.filter { !bad.contains($0) }))
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return t.count > limit ? String(t.prefix(limit)) + "…" : t
    }

    private static func string(_ v: JSONValue?) -> String? { if case .string(let s)? = v { return s }; return nil }

    /// Parses an `nl-ops` answer. Never throws: unknown shapes give an empty plan.
    public static func parse(_ json: JSONValue, role: String?) -> AIPlan {
        var plan = AIPlan.empty
        plan.intent = clean(string(json["intent"]) ?? "", limit: 80)
        plan.summary = clean(string(json["summary"]) ?? "")
        plan.reply = clean(string(json["reply"]) ?? "", limit: 4000)
        if case .number(let n)? = json["risk_score"] { plan.riskScore = max(0, min(100, Int(n))) }
        let steps = json["steps"]?.array ?? []
        let ids = (json["action_ids"]?.array ?? []).map { string($0).flatMap { UUID(uuidString: $0) } }
        let aligned = ids.count == steps.count            // ids line up with steps only when the counts match
        for (i, step) in steps.enumerated() {
            let label = clean(string(step["label"]) ?? "(unnamed step)", limit: 200)
            if plan.actions.count >= maxActions { plan.dropped.append(.init(label: label, reason: "too many steps")); continue }
            guard let type = string(step["action_type"]), let policy = AIAllowlist.policy(for: type) else {
                plan.dropped.append(.init(label: label, reason: "unknown action type")); continue
            }
            if let bad = disallowedEndpoint(in: step) { plan.dropped.append(.init(label: label, reason: bad)); continue }
            guard AIAllowlist.allows(policy, role: role) else {
                plan.dropped.append(.init(label: label, reason: role == "operator" ? "needs the admin role" : "your role cannot make changes")); continue
            }
            let risk = (string(step["risk"]) ?? "").lowercased()
            plan.actions.append(AIProposedAction(
                id: i, label: label, actionType: type, review: clean(string(step["review"]) ?? ""),
                risk: ["low", "medium", "high", "critical"].contains(risk) ? risk : "unknown",
                policy: policy, actionId: aligned ? ids[i] : nil))
        }
        return plan
    }

    /// A step that names its own endpoint, verb or URL must name an allowlisted one. Anything else is an injection attempt.
    static func disallowedEndpoint(in step: JSONValue) -> String? {
        let method = string(step["method"]) ?? string(step["verb"])
        for k in ["path", "endpoint", "url", "uri"] {
            guard let v = step[k] else { continue }
            guard let p = string(v) else { return "names an endpoint that is not allowed" }
            let path = p.split(separator: "?").first.map(String.init) ?? p
            if !AIAllowlist.permits(method: method ?? "POST", path: path) { return "names an endpoint that is not allowed (\(clean(p, limit: 60)))" }
        }
        if let m = method, !["POST", "GET"].contains(m.uppercased()) { return "uses a verb that is not allowed (\(clean(m, limit: 12)))" }
        return nil
    }
}

// MARK: Providers

public enum AIProviderState: Hashable, Sendable {
    case unknown
    case unavailable(String)
    case available(name: String, kind: String, model: String)

    public var isAvailable: Bool { if case .available = self { true } else { false } }
    public var label: String {
        switch self {
        case .unknown: "Checking AI provider…"
        case .unavailable(let why): why
        case .available(let n, _, let m): m.isEmpty ? n : "\(n) · \(m)"
        }
    }

    /// `GET /ai/providers` is an array of `{name, kind, deployment_name, enabled, is_default, api_key_configured}`.
    public static func parse(_ json: JSONValue) -> AIProviderState {
        let rows = (json.array ?? json["providers"]?.array ?? []).filter { $0.object != nil }
        let usable = rows.filter { r in
            if case .bool(false)? = r["enabled"] { return false }
            return true
        }
        guard !usable.isEmpty else { return .unavailable("AI unavailable: no provider configured. An admin can add one under AI providers.") }
        let pick = usable.first { if case .bool(true)? = $0["is_default"] { true } else { false } } ?? usable[0]
        func s(_ k: String) -> String { if case .string(let v)? = pick[k] { return AIPlanParser.clean(v, limit: 80) }; return "" }
        return .available(name: s("name"), kind: s("kind"), model: s("deployment_name"))
    }
}

// MARK: Conversation

public struct AIMessage: Codable, Hashable, Identifiable, Sendable {
    public enum Role: String, Codable, Sendable { case user, assistant, system }
    public var id: UUID
    public var role: Role
    public var text: String
    public var date: Date
    /// Who answered (agent and provider), shown under the reply.
    public var source: String?
    /// Action labels proposed with the reply (history keeps the labels only, never ids).
    public var actionLabels: [String]
    public init(role: Role, text: String, source: String? = nil, actionLabels: [String] = [], date: Date = Date(), id: UUID = UUID()) {
        self.id = id; self.role = role; self.text = text; self.source = source; self.actionLabels = actionLabels; self.date = date
    }
}

public enum AIRequest {
    public static let maxQueryLength = 2000

    /// The request body for `nl-ops`: the typed text and nothing else.
    public static func nlOpsBody(_ text: String) -> JSONValue {
        .object(["query": .string(String(text.trimmingCharacters(in: .whitespacesAndNewlines).prefix(maxQueryLength))), "dry_run": .bool(false)])
    }
    public static func chatBody(_ text: String) -> JSONValue {
        .object(["message": .string(String(text.trimmingCharacters(in: .whitespacesAndNewlines).prefix(maxQueryLength))), "agent": .string("auto")])
    }
}

/// Removes secret-looking text before anything is written to the local history.
public enum AIHistoryScrub {
    private static let patterns = [
        #"(?i)bearer\s+[A-Za-z0-9._~+/=-]{8,}"#,
        #"(?i)\b(password|passwd|pwd|secret|token|api[_-]?key|authorization|cookie)\b\s*[:=]\s*\S+"#,
        #"\bsk-[A-Za-z0-9_-]{10,}"#,
        #"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]*"#,
        #"-----BEGIN [A-Z ]*PRIVATE KEY-----"#,
    ]
    public static func scrub(_ s: String) -> String {
        patterns.reduce(s) { $0.replacingOccurrences(of: $1, with: "<redacted>", options: .regularExpression) }
    }
}

public struct AIHistoryStore: Sendable {
    public static let maxMessages = 200
    public let url: URL
    public init(url: URL) { self.url = url }

    public static var defaultURL: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first ?? FileManager.default.temporaryDirectory
        return base.appendingPathComponent("Machina", isDirectory: true).appendingPathComponent("ai-history.json")
    }

    public func load() -> [AIMessage] {
        guard let d = try? Data(contentsOf: url) else { return [] }
        let dec = JSONDecoder(); dec.dateDecodingStrategy = .iso8601
        return (try? dec.decode([AIMessage].self, from: d)) ?? []
    }

    /// Saves a scrubbed, capped copy. With `enabled == false` nothing is written and any old file is deleted.
    public func save(_ messages: [AIMessage], enabled: Bool) {
        guard enabled else { clear(); return }
        let kept = messages.suffix(Self.maxMessages).map { m -> AIMessage in
            var c = m; c.text = AIHistoryScrub.scrub(m.text); c.actionLabels = m.actionLabels.map(AIHistoryScrub.scrub); return c
        }
        let enc = JSONEncoder(); enc.dateEncodingStrategy = .iso8601
        guard let d = try? enc.encode(kept) else { return }
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? d.write(to: url, options: .atomic)
    }

    public func clear() { try? FileManager.default.removeItem(at: url) }
}

// MARK: Service

public struct AIAnswer: Sendable {
    public var plan: AIPlan
    public var reply: String
    public var source: String
}

public enum AIOperatorError: Error, LocalizedError, Equatable {
    case notAllowed(String)
    public var errorDescription: String? { if case .notAllowed(let w) = self { "Refused: \(w)" } else { nil } }
}

public enum AIOperator {
    /// Provider status from `/ai/providers`; a failure (no controller, 403) reads as unavailable.
    public static func providerState(_ client: MachinaClient) async -> AIProviderState {
        do { return AIProviderState.parse(try await client.call(.controllerGet("/api/v1/ai/providers"))) }
        catch { return .unavailable("AI unavailable: \(error.localizedDescription)") }
    }

    /// Sends what the user typed to `nl-ops` (the controller's planner). When that yields no usable plan the same
    /// text goes to the agent chat for a plain-language answer. The API has no streaming, so this is one request.
    public static func ask(_ text: String, client: MachinaClient, role: String?, provider: AIProviderState) async throws -> AIAnswer {
        guard role == "admin" || role == "operator" else { throw AIOperatorError.notAllowed("your role cannot use the AI operator") }
        let planJSON = try await client.call(Endpoint("POST", "/api/v1/ai/nl-ops", body: AIRequest.nlOpsBody(text), controller: true, timeout: 120))
        let plan = AIPlanParser.parse(planJSON, role: role)
        var reply = plan.reply.isEmpty ? plan.summary : plan.reply
        var source = "Rule planner"
        if plan.actions.isEmpty && provider.isAvailable {
            if let chat = try? await client.call(Endpoint("POST", "/api/v1/ai/zyra/chat", body: AIRequest.chatBody(text), controller: true, timeout: 120)),
               case .string(let r)? = chat["reply"], !r.isEmpty {
                reply = AIPlanParser.clean(r, limit: 6000)
                var agent = "agent"
                if case .string(let a)? = chat["agent_id"] { agent = AIPlanParser.clean(a, limit: 40) }
                source = "\(agent) · \(provider.label)"
                if case .bool(true)? = chat["deterministic"] { source = "\(agent) · rule-based, no model" }
            }
        } else if !plan.actions.isEmpty {
            source = "Rule planner (proposes, you approve)"
        }
        if reply.isEmpty { reply = plan.actions.isEmpty ? "I have nothing to propose for that." : "Review the proposed actions below." }
        return AIAnswer(plan: plan, reply: reply, source: source)
    }

    /// The request that approves (or dismisses) one queued action. Re-checks the allowlist and the role; throws before any send.
    public static func executeEndpoint(for a: AIProposedAction, role: String?) throws -> Endpoint { try endpoint(a, verb: "execute", role: role) }
    public static func rejectEndpoint(for a: AIProposedAction, role: String?) throws -> Endpoint { try endpoint(a, verb: "reject", role: role) }

    private static func endpoint(_ a: AIProposedAction, verb: String, role: String?) throws -> Endpoint {
        guard let policy = AIAllowlist.policy(for: a.actionType) else { throw AIOperatorError.notAllowed("unknown action type") }
        guard AIAllowlist.allows(policy, role: role) else { throw AIOperatorError.notAllowed("your role cannot do that") }
        guard let id = a.actionId else { throw AIOperatorError.notAllowed("this step was not queued for approval") }
        let e = Endpoint("POST", "/api/v1/ai/actions/\(id.uuidString.lowercased())/\(verb)", controller: true, timeout: 60)
        guard AIAllowlist.permits(method: e.method, path: e.path) else { throw AIOperatorError.notAllowed("endpoint not allowed") }
        return e
    }
}
