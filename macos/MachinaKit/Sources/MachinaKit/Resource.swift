import Foundation

/// A button on a page: runs one request against the selected row. `{key}` in `path` is replaced by the
/// row's value (path-encoded); `bodyFields` maps a body field name to a row key.
public struct ResourceAction: Hashable, Sendable, Identifiable {
    public var title: String
    public var method: String
    public var path: String
    public var destructive: Bool
    public var controller: Bool
    public var bodyFields: [String: String]
    public var id: String { title }
    /// POST by default.
    public init(_ title: String, _ path: String, destructive: Bool = false, controller: Bool = false, bodyFields: [String: String] = [:]) {
        self.init(title, "POST", path, destructive: destructive, controller: controller, bodyFields: bodyFields)
    }
    public init(_ title: String, _ method: String, _ path: String, destructive: Bool = false,
                controller: Bool = false, bodyFields: [String: String] = [:]) {
        self.title = title; self.method = method; self.path = path; self.destructive = destructive
        self.controller = controller; self.bodyFields = bodyFields
    }

    /// nil when a placeholder has no value in the row (so a bad request is never sent).
    public func endpoint(for row: ResourceRow) -> Endpoint? {
        var out = ""; var rest = Substring(path)
        while let open = rest.firstIndex(of: "{") {
            out += rest[..<open]
            guard let close = rest[open...].firstIndex(of: "}") else { return nil }
            let key = String(rest[rest.index(after: open)..<close])
            guard let v = row.values[key], v != .null else { return nil }
            out += MachinaClient.seg(v.text)
            rest = rest[rest.index(after: close)...]
        }
        out += rest
        var body: JSONValue?
        if !bodyFields.isEmpty {
            var o: [String: JSONValue] = [:]
            for (field, key) in bodyFields { guard let v = row.values[key] else { return nil }; o[field] = v }
            body = .object(o)
        }
        return Endpoint(method, out, body: body, controller: controller)
    }
}

public struct ResourceRow: Identifiable, Hashable, Sendable {
    public var id: String
    public var values: JSONValue
}

public struct ResourceColumn: Hashable, Sendable, Identifiable {
    public enum Format: Hashable, Sendable { case text, gigabytes, percent, date }
    public var title: String
    public var key: String
    public var format: Format
    public var id: String { key }
    public init(_ title: String, _ key: String, _ format: Format = .text) { self.title = title; self.key = key; self.format = format }

    public func render(_ row: ResourceRow) -> String {
        guard let v = row.values[key] else { return "–" }
        switch format {
        case .text, .date: return v.text
        case .gigabytes: return v.double.map { String(format: "%.1f GB", $0) } ?? v.text
        case .percent: return v.double.map { String(format: "%.0f%%", $0) } ?? v.text
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

    public init(_ id: String, _ title: String, group: String, icon: String, endpoint: Endpoint, rowsPath: [String] = [],
                idKey: String? = nil, columns: [ResourceColumn] = [], layout: Layout = .table, actions: [ResourceAction] = []) {
        self.id = id; self.title = title; self.group = group; self.icon = icon; self.endpoint = endpoint
        self.rowsPath = rowsPath; self.idKey = idKey; self.columns = columns; self.layout = layout; self.actions = actions
    }

    public func rows(from json: JSONValue) -> [ResourceRow] {
        var node: JSONValue? = json
        for k in rowsPath { node = node?[k] }
        if layout == .keyValue, let o = (node ?? json).object {
            return o.keys.sorted().map { k in ResourceRow(id: k, values: .object(["key": .string(k), "value": o[k]!])) }
        }
        let items = node?.array ?? (node?.object != nil ? [node!] : [])
        return items.enumerated().map { i, v in
            ResourceRow(id: idKey.flatMap { v[$0]?.text } ?? String(i), values: v)
        }
    }

    public func effectiveColumns(for rows: [ResourceRow]) -> [ResourceColumn] {
        if layout == .keyValue { return [ResourceColumn("Key", "key"), ResourceColumn("Value", "value")] }
        if !columns.isEmpty { return columns }
        guard let first = rows.first?.values.object else { return [] }
        return first.keys.sorted().filter { first[$0]!.isScalar }.prefix(6).map { ResourceColumn($0.replacingOccurrences(of: "_", with: " ").capitalized, $0) }
    }
}
