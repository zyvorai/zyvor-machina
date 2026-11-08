import Foundation

public enum MachinaError: Error, LocalizedError, Sendable {
    case http(Int, String), badURL, unauthenticated, timedOut(String, Int)
    public var errorDescription: String? {
        switch self {
        case .http(let c, let m): return "HTTP \(c): \(m)"
        case .badURL: return "Invalid server URL"
        case .unauthenticated: return "Sign in required"
        case .timedOut(let path, let secs):
            return "No answer from \(path) after \(secs) s. The controller may be busy building this report; try again in a minute. Nothing was changed."
        }
    }
}

/// Daemon REST client. Session cookie (`machina_session`) lives in the session's cookie storage;
/// a 401 triggers one re-login with the stored credentials.
public actor MachinaClient {
    public let server: ServerProfile
    private let session: URLSession
    private var credentials: (user: String, password: String)?
    private let decoder = JSONDecoder()

    public init(server: ServerProfile, session: URLSession) {
        self.server = server; self.session = session
    }

    public static func makeSession(delegate: URLSessionDelegate?) -> URLSession {
        // Ephemeral keeps its own working cookie store (a bare HTTPCookieStorage() is not usable
        // by URLSession), so the machina_session cookie survives between requests per server.
        let cfg = URLSessionConfiguration.ephemeral
        cfg.httpShouldSetCookies = true
        cfg.httpCookieAcceptPolicy = .always
        cfg.timeoutIntervalForRequest = 20
        return URLSession(configuration: cfg, delegate: delegate, delegateQueue: nil)
    }

    private func url(_ path: String, _ query: [URLQueryItem] = []) throws -> URL {
        guard var c = URLComponents(url: server.url, resolvingAgainstBaseURL: false) else { throw MachinaError.badURL }
        c.percentEncodedPath = path   // callers pass an already-encoded path
        c.queryItems = query.isEmpty ? nil : query
        guard let u = c.url else { throw MachinaError.badURL }
        return u
    }

    func send(_ method: String, _ path: String, query: [URLQueryItem] = [],
                      body: Data? = nil, retry: Bool = true, timeout: TimeInterval? = nil) async throws -> Data {
        var req = URLRequest(url: try url(path, query))
        req.httpMethod = method
        if let timeout { req.timeoutInterval = timeout }
        if let body { req.httpBody = body; req.setValue("application/json", forHTTPHeaderField: "Content-Type") }
        let (data, resp) = try await session.data(for: req)
        let code = (resp as? HTTPURLResponse)?.statusCode ?? 0
        if code == 401, retry, let c = credentials, path != "/api/v1/auth/login" {
            try await login(user: c.user, password: c.password)
            return try await send(method, path, query: query, body: body, retry: false, timeout: timeout)
        }
        guard (200..<300).contains(code) else {
            throw code == 401 ? MachinaError.unauthenticated
                : MachinaError.http(code, String(data: data.prefix(300), encoding: .utf8) ?? "")
        }
        return data
    }

    @discardableResult
    public func login(user: String, password: String) async throws -> SessionInfo {
        credentials = (user, password)
        let body = try JSONSerialization.data(withJSONObject: ["username": user, "password": password])
        let data = try await send("POST", "/api/v1/auth/login", body: body, retry: false)
        return (try? decoder.decode(SessionInfo.self, from: data)) ?? SessionInfo(authenticated: true, username: user, role: nil)
    }

    public func logout() async {
        _ = try? await send("POST", "/api/v1/auth/logout", retry: false)
        credentials = nil
    }

    public func listVMs() async throws -> [VmInfo] {
        try decoder.decode([VmInfo].self, from: try await send("GET", "/api/v1/vms"))
    }

    public func power(_ action: PowerAction, vm: VmInfo) async throws {
        var segment = CharacterSet.urlPathAllowed; segment.remove("/")   // a "/" in a name must not add a path segment
        let name = vm.name.addingPercentEncoding(withAllowedCharacters: segment) ?? vm.name
        _ = try await send("POST", "/api/v1/vms/\(name)/\(action.rawValue)", query: vm.scopeQuery)
    }

    /// Single-use token (120 s) for `/ws/v1/watch`; fetch a fresh one per connection.
    public func wsToken() async throws -> String {
        try decoder.decode(WsToken.self, from: try await send("POST", "/api/v1/ws-token")).token
    }

    public func watchURL(token: String) throws -> URL {
        var c = URLComponents(url: server.url, resolvingAgainstBaseURL: false)
        c?.scheme = server.url.scheme == "http" ? "ws" : "wss"
        c?.path = "/ws/v1/watch"
        c?.queryItems = [URLQueryItem(name: "token", value: token)]
        guard let u = c?.url else { throw MachinaError.badURL }
        return u
    }

    public var urlSession: URLSession { session }
}
