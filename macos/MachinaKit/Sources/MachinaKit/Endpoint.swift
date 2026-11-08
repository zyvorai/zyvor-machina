import Foundation

/// One REST call. `controller` routes go through the daemon's reverse proxy (never :5093 directly).
public struct Endpoint: Hashable, Sendable {
    public var method: String
    public var path: String
    public var query: [URLQueryItem]
    public var body: JSONValue?
    public var viaController: Bool
    /// Per-request timeout in seconds; nil keeps the session default (20 s). Slow controller reports set this.
    public var timeout: TimeInterval?

    public init(_ method: String = "GET", _ path: String, query: [URLQueryItem] = [], body: JSONValue? = nil, controller: Bool = false,
                timeout: TimeInterval? = nil) {
        self.method = method; self.path = path; self.query = query; self.body = body; self.viaController = controller; self.timeout = timeout
    }
    public static func get(_ path: String, query: [URLQueryItem] = []) -> Endpoint { Endpoint("GET", path, query: query) }
    public static func controllerGet(_ path: String, query: [URLQueryItem] = []) -> Endpoint {
        Endpoint("GET", path, query: query, controller: true)
    }
    public var resolvedPath: String { viaController ? "/api/v1/platform/controller" + path : path }
}

public extension MachinaClient {
    /// Generic call returning dynamic JSON. A body that starts like HTML means the path is not an API
    /// route (the web app's index page is served for unknown paths), so it is reported as a 404.
    func call(_ e: Endpoint) async throws -> JSONValue {
        try SecretRedaction.requireTLS(body: e.body, server: server.url)
        let body = try e.body.map { try JSONEncoder().encode($0) }
        let data: Data
        do { data = try await send(e.method, e.resolvedPath, query: e.query, body: body, timeout: e.timeout) }
        catch let u as URLError where u.code == .timedOut {
            throw MachinaError.timedOut(e.path, Int(e.timeout ?? 20))
        }
        if data.isEmpty { return .null }
        let head = String(decoding: data.prefix(15), as: UTF8.self).lowercased()
        if head.hasPrefix("<!doctype") || head.hasPrefix("<html") { throw MachinaError.http(404, "Not an API route: \(e.path)") }
        // Some write endpoints answer with plain text; keep it as a string instead of failing.
        return (try? JSONDecoder().decode(JSONValue.self, from: data)) ?? .string(String(decoding: data, as: UTF8.self))
    }

    func sessionInfo() async throws -> SessionInfo {
        try JSONDecoder().decode(SessionInfo.self, from: try await send("GET", "/api/v1/auth/session"))
    }
}
