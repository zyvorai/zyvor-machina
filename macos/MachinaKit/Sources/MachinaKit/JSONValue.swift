import Foundation

/// Dynamic JSON for endpoints whose schema is loose or drifts (the OpenAPI files carry no response schemas).
public enum JSONValue: Codable, Hashable, Sendable {
    case null, bool(Bool), number(Double), string(String), array([JSONValue]), object([String: JSONValue])

    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let v = try? c.decode(Bool.self) { self = .bool(v) }
        else if let v = try? c.decode(Double.self) { self = .number(v) }
        else if let v = try? c.decode(String.self) { self = .string(v) }
        else if let v = try? c.decode([JSONValue].self) { self = .array(v) }
        else { self = .object(try c.decode([String: JSONValue].self)) }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let v): try c.encode(v)
        case .number(let v): try c.encode(v)
        case .string(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .object(let v): try c.encode(v)
        }
    }

    public subscript(key: String) -> JSONValue? { if case .object(let o) = self { return o[key] }; return nil }
    /// Dotted lookup, e.g. `metadata.name` or `status.phase`; a missing step gives nil.
    public func value(at path: String) -> JSONValue? {
        var node: JSONValue? = self
        for k in path.split(separator: ".") { node = node?[String(k)] }
        return node
    }
    public var array: [JSONValue]? { if case .array(let a) = self { return a }; return nil }
    public var object: [String: JSONValue]? { if case .object(let o) = self { return o }; return nil }
    public var isScalar: Bool { switch self { case .array, .object: false; default: true } }
    public var double: Double? {
        switch self { case .number(let n): n; case .string(let s): Double(s); case .bool(let b): b ? 1 : 0; default: nil }
    }

    /// Plain text for tables: integers without decimals, arrays as a count or joined scalars, objects as `{n keys}`.
    public var text: String {
        switch self {
        case .null: return "–"
        case .bool(let b): return b ? "yes" : "no"
        case .number(let n): return n == n.rounded() && abs(n) < 1e15 ? String(Int64(n)) : String(format: "%g", n)
        case .string(let s): return s
        case .array(let a): return a.isEmpty ? "–" : a.allSatisfy(\.isScalar) && a.count <= 4 ? a.map(\.text).joined(separator: ", ") : "\(a.count) items"
        case .object(let o): return "{\(o.count) keys}"
        }
    }
}
