import Foundation

/// A "New…" form: typed fields turned into a JSON body. Int fields must be whole numbers; empty optional fields are left out.
public struct ResourceCreate: Hashable, Sendable {
    public enum Kind: Hashable, Sendable { case text, int, bool }
    public struct Field: Hashable, Sendable, Identifiable {
        public var id: String          // JSON key
        public var label: String
        public var kind: Kind
        public var initial: String
        public var required: Bool
        public init(_ id: String, _ label: String, _ kind: Kind = .text, initial: String = "", required: Bool = true) {
            self.id = id; self.label = label; self.kind = kind; self.initial = initial; self.required = required
        }
    }
    public struct Invalid: Error, LocalizedError, Sendable { public var message: String; public var errorDescription: String? { message } }

    public var title: String
    public var path: String
    public var controller: Bool
    public var fields: [Field]
    public init(_ title: String, _ path: String, controller: Bool = true, fields: [Field]) {
        self.title = title; self.path = path; self.controller = controller; self.fields = fields
    }

    public func body(from values: [String: String]) throws -> JSONValue {
        var o: [String: JSONValue] = [:]
        for f in fields {
            let raw = (values[f.id] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            if raw.isEmpty { if f.required { throw Invalid(message: "\(f.label) is required.") }; continue }
            switch f.kind {
            case .text: o[f.id] = .string(raw)
            case .int:
                guard let n = Int(raw), n >= 0 else { throw Invalid(message: "\(f.label) must be a whole number.") }
                o[f.id] = .number(Double(n))
            case .bool: o[f.id] = .bool(raw == "yes" || raw == "true")
            }
        }
        return .object(o)
    }
    public var endpoint: Endpoint { Endpoint("POST", path, controller: controller) }
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
    public init(parent: Endpoint, parentRowsPath: [String] = [], idKey: String = "id", labelKey: String = "name", childPath: String, controller: Bool = true) {
        self.parent = parent; self.parentRowsPath = parentRowsPath; self.idKey = idKey; self.labelKey = labelKey
        self.childPath = childPath; self.controller = controller
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
    public var id: String { title }
    /// POST by default.
    public init(_ title: String, _ path: String, destructive: Bool = false, controller: Bool = false,
                bodyFields: [String: String] = [:], query: [URLQueryItem] = [], staticBody: JSONValue? = nil) {
        self.init(title, "POST", path, destructive: destructive, controller: controller, bodyFields: bodyFields, query: query, staticBody: staticBody)
    }
    public init(_ title: String, _ method: String, _ path: String, destructive: Bool = false, controller: Bool = false,
                bodyFields: [String: String] = [:], query: [URLQueryItem] = [], staticBody: JSONValue? = nil) {
        self.title = title; self.method = method; self.path = path; self.destructive = destructive
        self.controller = controller; self.bodyFields = bodyFields; self.query = query; self.staticBody = staticBody
    }

    /// nil when a placeholder has no value in the row (so a bad request is never sent).
    public func endpoint(for row: ResourceRow) -> Endpoint? {
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
        return Endpoint(method, out, query: query, body: body, controller: controller)
    }
}

public struct ResourceRow: Identifiable, Hashable, Sendable {
    public var id: String
    public var values: JSONValue
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
    public var fanOut: FanOut?
    public var transform: RowTransform?

    public init(_ id: String, _ title: String, group: String, icon: String, endpoint: Endpoint, rowsPath: [String] = [],
                idKey: String? = nil, columns: [ResourceColumn] = [], layout: Layout = .table, actions: [ResourceAction] = [],
                create: ResourceCreate? = nil, fanOut: FanOut? = nil, transform: RowTransform? = nil) {
        self.id = id; self.title = title; self.group = group; self.icon = icon; self.endpoint = endpoint
        self.rowsPath = rowsPath; self.idKey = idKey; self.columns = columns; self.layout = layout; self.actions = actions
        self.create = create; self.fanOut = fanOut; self.transform = transform
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
        let parents = Array((node.array ?? []).prefix(50))
        guard !parents.isEmpty else { return .array([]) }
        let results: [Result<[JSONValue], Error>] = await withTaskGroup(of: (Int, Result<[JSONValue], Error>).self) { group in
            for (i, p) in parents.enumerated() {
                guard let id = p[f.idKey]?.text else { continue }
                let label = p[f.labelKey]?.text ?? id
                let path = f.childPath.replacingOccurrences(of: "{id}", with: MachinaClient.seg(id))
                group.addTask { [self] in
                    do {
                        let v = try await call(Endpoint("GET", path, controller: f.controller))
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
