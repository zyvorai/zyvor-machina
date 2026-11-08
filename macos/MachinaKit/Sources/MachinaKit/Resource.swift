import Foundation

/// A "New…" form: typed fields turned into a JSON body. Int fields must be whole numbers; empty optional fields are left out.
public struct ResourceCreate: Hashable, Sendable {
    /// `list` / `intList`: comma or space separated values sent as a JSON array.
    public enum Kind: Hashable, Sendable { case text, int, bool, list, intList }
    public struct Field: Hashable, Sendable, Identifiable {
        public var id: String          // JSON key
        public var label: String
        public var kind: Kind
        public var initial: String
        public var required: Bool
        /// When set, the form shows a picker filled from this endpoint instead of free text.
        public var choices: Choices?
        /// Inclusive bounds for `.int` fields (checked before anything is sent).
        public var range: ClosedRange<Int>?
        /// Allowed values for a `.text` field (shown as a picker).
        public var options: [String]
        public init(_ id: String, _ label: String, _ kind: Kind = .text, initial: String = "", required: Bool = true,
                    choices: Choices? = nil, range: ClosedRange<Int>? = nil, options: [String] = []) {
            self.id = id; self.label = label; self.kind = kind; self.initial = initial; self.required = required
            self.choices = choices; self.range = range; self.options = options
        }
    }
    /// Picker options loaded from a list endpoint: the value is `valueKeys` joined with `valueJoin`, the label `labelKeys` joined with " · ".
    public struct Choices: Hashable, Sendable {
        public var endpoint: Endpoint
        public var rowsPath: [String]
        public var valueKeys: [String]
        public var valueJoin: String
        public var labelKeys: [String]
        public init(_ endpoint: Endpoint, rowsPath: [String] = [], valueKeys: [String], valueJoin: String = "@", labelKeys: [String]) {
            self.endpoint = endpoint; self.rowsPath = rowsPath; self.valueKeys = valueKeys; self.valueJoin = valueJoin; self.labelKeys = labelKeys
        }
        public func options(from json: JSONValue) -> [(label: String, value: String)] {
            var node: JSONValue? = json
            for k in rowsPath { node = node?[k] }
            return (node?.array ?? []).compactMap { row in
                let parts = valueKeys.compactMap { row.value(at: $0).map { $0.text } }
                guard parts.count == valueKeys.count, !parts.contains("") else { return nil }
                let label = labelKeys.compactMap { row.value(at: $0).map { $0.text } }.filter { !$0.isEmpty }.joined(separator: " · ")
                return (label.isEmpty ? parts.joined(separator: valueJoin) : label, parts.joined(separator: valueJoin))
            }
        }
    }
    public struct Invalid: Error, LocalizedError, Sendable { public var message: String; public var errorDescription: String? { message } }

    public var title: String
    public var path: String
    public var controller: Bool
    public var fields: [Field]
    public var method: String = "POST"
    /// Body fields the user cannot change (for example `dry_run: true`), merged after the typed ones.
    public var fixed: [String: JSONValue] = [:]
    /// Shown in the form and in the confirmation that follows it.
    public var warning: String?
    /// The user must type this exact phrase in the form (an extra confirmation for dangerous writes).
    public var typedPhrase: String?
    /// Needs the admin role (the server enforces it too).
    public var adminOnly = false
    /// Refuse a request that could cut off the operator: `allow_tcp` without 22 and no `exempt` entry.
    public var guardLockout = false
    public var timeout: TimeInterval?
    /// After success, show this key of the JSON answer (for example a chat `reply`).
    public var resultKey: String?
    public init(_ title: String, _ path: String, controller: Bool = true, fields: [Field]) {
        self.title = title; self.path = path; self.controller = controller; self.fields = fields
    }
    /// Builder-style tweaks so specs stay one expression.
    public func with(_ edit: (inout ResourceCreate) -> Void) -> ResourceCreate { var c = self; edit(&c); return c }

    /// Key of the extra typed-confirmation field in the values dictionary.
    public static let phraseKey = "_confirm"

    public func body(from values: [String: String]) throws -> JSONValue {
        if let phrase = typedPhrase, (values[Self.phraseKey] ?? "").trimmingCharacters(in: .whitespacesAndNewlines) != phrase {
            throw Invalid(message: "Type \(phrase) exactly to confirm.")
        }
        var o: [String: JSONValue] = [:]
        for f in fields {
            let raw = (values[f.id] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            if raw.isEmpty { if f.required { throw Invalid(message: "\(f.label) is required.") }; continue }
            switch f.kind {
            case .text:
                if !f.options.isEmpty, !f.options.contains(raw) { throw Invalid(message: "\(f.label) must be one of \(f.options.joined(separator: ", ")).") }
                o[f.id] = .string(raw)
            case .int:
                guard let n = Int(raw), n >= 0 else { throw Invalid(message: "\(f.label) must be a whole number.") }
                if let r = f.range, !r.contains(n) { throw Invalid(message: "\(f.label) must be between \(r.lowerBound) and \(r.upperBound).") }
                o[f.id] = .number(Double(n))
            case .bool: o[f.id] = .bool(raw == "yes" || raw == "true")
            case .list: o[f.id] = .array(Self.split(raw).map(JSONValue.string))
            case .intList:
                let parts = Self.split(raw)
                let nums = parts.compactMap { Int($0) }.filter { (1...65535).contains($0) }
                guard nums.count == parts.count else { throw Invalid(message: "\(f.label) must be port numbers (1 to 65535) separated by commas.") }
                o[f.id] = .array(nums.map { .number(Double($0)) })
            }
        }
        for (k, v) in fixed { o[k] = v }
        if guardLockout {
            let ports = o["allow_tcp"]?.array?.compactMap { $0.double.map(Int.init) } ?? []
            let exempt = o["exempt"]?.array ?? []
            if o["dry_run"] != .bool(true), !ports.contains(22), exempt.isEmpty {
                throw Invalid(message: "Allow TCP 22 or add an exempt CIDR (your own address), or this could lock you out.")
            }
        }
        return .object(o)
    }
    static func split(_ raw: String) -> [String] { raw.split(whereSeparator: { $0 == "," || $0 == " " }).map(String.init) }
    public var endpoint: Endpoint { Endpoint(method, path, controller: controller, timeout: timeout) }
}

/// Lists a parent resource, then fetches each parent's children (`{id}` in `childPath`) and merges them; every
/// child row gets a `_parent` field with the parent's label. One failing parent is skipped, all failing is an error.
public struct FanOut: Hashable, Sendable {
    public var parent: Endpoint
    public var parentRowsPath: [String]
    public var idKey: String
    public var labelKey: String
    public var childPath: String
    public var controller: Bool
    /// Query for each child call; `{id}` in a value is replaced by the parent's id.
    public var childQuery: [URLQueryItem]
    /// Where the child rows are inside each child answer (empty: the answer is the array or `{items}`).
    public var childRowsPath: [String]
    /// Only parents whose `filterKey` equals `filterValue`.
    public var filterKey: String?
    public var filterValue: String?
    public var limit: Int
    public var childTimeout: TimeInterval?
    public init(parent: Endpoint, parentRowsPath: [String] = [], idKey: String = "id", labelKey: String = "name", childPath: String, controller: Bool = true,
                childQuery: [URLQueryItem] = [], childRowsPath: [String] = [], filterKey: String? = nil, filterValue: String? = nil,
                limit: Int = 50, childTimeout: TimeInterval? = nil) {
        self.parent = parent; self.parentRowsPath = parentRowsPath; self.idKey = idKey; self.labelKey = labelKey
        self.childPath = childPath; self.controller = controller; self.childQuery = childQuery; self.childRowsPath = childRowsPath
        self.filterKey = filterKey; self.filterValue = filterValue; self.limit = limit; self.childTimeout = childTimeout
    }
}

public enum RowTransform: Hashable, Sendable {
    /// Server groups are not stored: a group is every `anti-affinity:<name>` tag in use across the VM list.
    case antiAffinityGroups

    func apply(_ json: JSONValue) -> JSONValue {
        switch self {
        case .antiAffinityGroups:
            var groups: [String: [String]] = [:]
            for vm in json.array ?? [] {
                let name = vm["name"]?.text ?? "?"
                for t in vm["tags"]?.array ?? [] where t.text.hasPrefix("anti-affinity:") {
                    groups[String(t.text.dropFirst("anti-affinity:".count)), default: []].append(name)
                }
            }
            return .array(groups.keys.sorted().map { g in
                .object(["group": .string(g), "members": .number(Double(groups[g]!.count)), "vms": .string(groups[g]!.sorted().joined(separator: ", "))])
            })
        }
    }
}

/// A button on a page: runs one request against the selected row. `{key}` in `path` is replaced by the
/// row's value (path-encoded); `bodyFields` maps a body field name to a row key.
public struct ResourceAction: Hashable, Sendable, Identifiable {
    public var title: String
    public var method: String
    public var path: String
    public var destructive: Bool
    public var controller: Bool
    public var bodyFields: [String: String]
    public var query: [URLQueryItem]
    public var staticBody: JSONValue?
    /// Values asked for before the request is sent (merged into the body; whole numbers for `.int`).
    public var inputs: [ResourceCreate.Field]
    /// Text for the confirmation dialog; falls back to a generic warning. Shown even when not destructive.
    public var confirm: String?
    /// Runs without a selected row (a page-level action).
    public var global: Bool
    /// The row key whose value the user must type to confirm (for example the experiment name).
    public var typeToConfirm: String?
    /// Needs the admin role (BMC power, enforcement). The server enforces it too.
    public var adminOnly = false
    public var timeout: TimeInterval?
    public var id: String { title }
    public func with(_ edit: (inout ResourceAction) -> Void) -> ResourceAction { var a = self; edit(&a); return a }

    /// The phrase to type for this row, nil when the action needs none (or the row has no value).
    public func phrase(for row: ResourceRow) -> String? {
        guard let k = typeToConfirm else { return nil }
        return row.values.value(at: k)?.text
    }
    /// POST by default.
    public init(_ title: String, _ path: String, destructive: Bool = false, controller: Bool = false,
                bodyFields: [String: String] = [:], query: [URLQueryItem] = [], staticBody: JSONValue? = nil,
                inputs: [ResourceCreate.Field] = [], confirm: String? = nil, global: Bool = false) {
        self.init(title, "POST", path, destructive: destructive, controller: controller, bodyFields: bodyFields, query: query, staticBody: staticBody,
                  inputs: inputs, confirm: confirm, global: global)
    }
    public init(_ title: String, _ method: String, _ path: String, destructive: Bool = false, controller: Bool = false,
                bodyFields: [String: String] = [:], query: [URLQueryItem] = [], staticBody: JSONValue? = nil,
                inputs: [ResourceCreate.Field] = [], confirm: String? = nil, global: Bool = false) {
        self.inputs = inputs; self.confirm = confirm; self.global = global
        self.title = title; self.method = method; self.path = path; self.destructive = destructive
        self.controller = controller; self.bodyFields = bodyFields; self.query = query; self.staticBody = staticBody
    }

    /// nil when a placeholder has no value in the row (so a bad request is never sent).
    public func endpoint(for row: ResourceRow, inputValues: [String: JSONValue] = [:]) -> Endpoint? {
        var out = ""; var rest = Substring(path)
        while let open = rest.firstIndex(of: "{") {
            out += rest[..<open]
            guard let close = rest[open...].firstIndex(of: "}") else { return nil }
            let key = String(rest[rest.index(after: open)..<close])
            guard let v = row.values.value(at: key), v != .null else { return nil }
            out += MachinaClient.seg(v.text)
            rest = rest[rest.index(after: close)...]
        }
        out += rest
        var body: JSONValue? = staticBody
        if !bodyFields.isEmpty {
            var o: [String: JSONValue] = staticBody?.object ?? [:]
            for (field, key) in bodyFields { guard let v = row.values.value(at: key) else { return nil }; o[field] = v }
            body = .object(o)
        }
        if !inputValues.isEmpty {
            var o = body?.object ?? [:]
            for (k, v) in inputValues { o[k] = v }
            body = .object(o)
        }
        return Endpoint(method, out, query: query, body: body, controller: controller, timeout: timeout)
    }

    /// Typed body values from the raw text the user entered for `inputs`.
    public func inputBody(from values: [String: String]) throws -> [String: JSONValue] {
        guard !inputs.isEmpty else { return [:] }
        let form = ResourceCreate(title, path, fields: inputs)
        return try form.body(from: values).object ?? [:]
    }

    /// Some endpoints (kubectl wrappers) answer 200 with `ok: false`; the message to show then.
    public static func failure(in json: JSONValue) -> String? {
        guard json["ok"] == .bool(false) else { return nil }
        let err = json["stderr"]?.text ?? ""
        return err.isEmpty ? (json["error"]?.text ?? "the command failed (exit \(json["exit_code"]?.text ?? "?"))") : err.trimmingCharacters(in: .whitespacesAndNewlines)
    }
    /// A short result line when the response carries one (`summary`, e.g. a DRS dry run).
    public static func summary(of json: JSONValue) -> String? {
        let t = json["summary"]?.text ?? ""
        return t.isEmpty ? nil : t
    }
}

public struct ResourceRow: Identifiable, Hashable, Sendable {
    public var id: String
    public var values: JSONValue
    public init(id: String, values: JSONValue) { self.id = id; self.values = values }
}

public struct ResourceColumn: Hashable, Sendable, Identifiable {
    public enum Format: Hashable, Sendable { case text, gigabytes, percent, date, bytes }
    public var title: String
    public var key: String
    public var format: Format
    public var id: String { key }
    public init(_ title: String, _ key: String, _ format: Format = .text) { self.title = title; self.key = key; self.format = format }

    public func render(_ row: ResourceRow) -> String {
        guard let v = row.values.value(at: key) else { return "–" }
        switch format {
        case .text, .date: return v.text
        case .gigabytes: return v.double.map { String(format: "%.1f GB", $0) } ?? v.text
        case .percent: return v.double.map { String(format: "%.0f%%", $0) } ?? v.text
        case .bytes: return v.double.map { ByteCountFormatter.string(fromByteCount: Int64($0), countStyle: .binary) } ?? v.text
        }
    }
}

/// A read-only page: one endpoint, where its rows are, and how to show them. Empty `columns` means
/// "derive from the first rows", so long-tail pages need only an endpoint.
public struct ResourceSpec: Identifiable, Hashable, Sendable {
    public enum Layout: Hashable, Sendable { case table, keyValue }
    public var id: String
    public var title: String
    public var group: String
    public var icon: String
    public var endpoint: Endpoint
    public var rowsPath: [String]
    public var idKey: String?
    public var columns: [ResourceColumn]
    public var layout: Layout
    public var actions: [ResourceAction]
    public var create: ResourceCreate?
    /// More "New…" style forms shown next to `create` (for example Dry run and Arm).
    public var moreCreates: [ResourceCreate]
    public var fanOut: FanOut?
    public var transform: RowTransform?
    /// Seconds between automatic reloads; 0 = only when the user presses reload (heavy reports).
    public var refreshSecs: Int

    public var allCreates: [ResourceCreate] { (create.map { [$0] } ?? []) + moreCreates }

    public init(_ id: String, _ title: String, group: String, icon: String, endpoint: Endpoint, rowsPath: [String] = [],
                idKey: String? = nil, columns: [ResourceColumn] = [], layout: Layout = .table, actions: [ResourceAction] = [],
                create: ResourceCreate? = nil, moreCreates: [ResourceCreate] = [], fanOut: FanOut? = nil, transform: RowTransform? = nil,
                refreshSecs: Int = 15) {
        self.id = id; self.title = title; self.group = group; self.icon = icon; self.endpoint = endpoint
        self.rowsPath = rowsPath; self.idKey = idKey; self.columns = columns; self.layout = layout; self.actions = actions
        self.create = create; self.moreCreates = moreCreates; self.fanOut = fanOut; self.transform = transform; self.refreshSecs = refreshSecs
    }

    public func rows(from json: JSONValue) -> [ResourceRow] {
        var node: JSONValue? = json
        for k in rowsPath { node = node?[k] }
        if layout == .keyValue, let o = (node ?? json).object {
            var flat: [(String, JSONValue)] = []
            func walk(_ prefix: String, _ v: JSONValue, _ depth: Int) {
                if depth < 2, let obj = v.object, !obj.isEmpty { for k in obj.keys.sorted() { walk(prefix + "." + k, obj[k]!, depth + 1) } }
                else { flat.append((prefix, v)) }
            }
            for k in o.keys.sorted() { walk(k, o[k]!, 0) }
            return flat.map { k, v in ResourceRow(id: k, values: .object(["key": .string(k), "value": v])) }
        }
        let items = node?.array ?? (node?.object != nil ? [node!] : [])
        return items.enumerated().map { i, v in
            ResourceRow(id: idKey.flatMap { v.value(at: $0)?.text } ?? String(i), values: v)
        }
    }

    public func effectiveColumns(for rows: [ResourceRow]) -> [ResourceColumn] {
        if layout == .keyValue { return [ResourceColumn("Key", "key"), ResourceColumn("Value", "value")] }
        if !columns.isEmpty { return columns }
        guard let first = rows.first?.values.object else { return [] }
        return first.keys.sorted().filter { first[$0]!.isScalar }.prefix(6).map { ResourceColumn($0 == "_parent" ? "Parent" : $0.replacingOccurrences(of: "_", with: " ").capitalized, $0) }
    }
}


public extension MachinaClient {
    /// Loads a page's data: a plain call, a derived transform, or a fan-out over parents.
    func load(_ spec: ResourceSpec) async throws -> JSONValue {
        guard let f = spec.fanOut else {
            let json = try await call(spec.endpoint)
            return spec.transform?.apply(json) ?? json
        }
        var node: JSONValue = try await call(f.parent)
        for k in f.parentRowsPath { node = node[k] ?? .null }
        var all = node.array ?? []
        if let k = f.filterKey, let want = f.filterValue { all = all.filter { $0[k]?.text == want } }
        let parents = Array(all.prefix(f.limit))
        guard !parents.isEmpty else { return .array([]) }
        let results: [Result<[JSONValue], Error>] = await withTaskGroup(of: (Int, Result<[JSONValue], Error>).self) { group in
            for (i, p) in parents.enumerated() {
                guard let id = p[f.idKey]?.text else { continue }
                let label = p[f.labelKey]?.text ?? id
                let path = f.childPath.replacingOccurrences(of: "{id}", with: MachinaClient.seg(id))
                group.addTask { [self] in
                    do {
                        let query = f.childQuery.map { URLQueryItem(name: $0.name, value: $0.value?.replacingOccurrences(of: "{id}", with: id)) }
                        var v = try await call(Endpoint("GET", path, query: query, controller: f.controller, timeout: f.childTimeout))
                        for k in f.childRowsPath { v = v[k] ?? .null }
                        let rows = v.array ?? v["items"]?.array ?? []
                        return (i, .success(rows.map { r in
                            var o = r.object ?? ["value": r]; o["_parent"] = .string(label); return .object(o)
                        }))
                    } catch { return (i, .failure(error)) }
                }
            }
            var out = [Result<[JSONValue], Error>](repeating: .success([]), count: parents.count)
            for await (i, r) in group { out[i] = r }
            return out
        }
        var rows: [JSONValue] = []; var firstError: Error?; var ok = 0
        for r in results { switch r { case .success(let v): rows += v; ok += 1; case .failure(let e): firstError = firstError ?? e } }
        if ok == 0, let e = firstError { throw e }
        return .array(rows)
    }
}
