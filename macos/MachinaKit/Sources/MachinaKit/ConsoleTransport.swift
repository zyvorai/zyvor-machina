import Foundation

/// A byte pipe to a console endpoint. The real one is a WebSocket; tests use a scripted one.
public protocol ConsoleTransport: Sendable {
    func receive() async throws -> Data
    func send(_ data: Data) async throws
    func close()
}

public final class WebSocketTransport: ConsoleTransport, @unchecked Sendable {
    private let task: URLSessionWebSocketTask
    /// `text` sends frames as WebSocket text (serial console); otherwise binary (VNC).
    private let text: Bool

    public init(session: URLSession, url: URL, protocols: [String], text: Bool) {
        task = session.webSocketTask(with: url, protocols: protocols)
        self.text = text
        task.resume()
    }

    public func receive() async throws -> Data {
        switch try await task.receive() {
        case .data(let d): return d
        case .string(let s): return Data(s.utf8)
        @unknown default: return Data()
        }
    }

    public func send(_ data: Data) async throws {
        if text, let s = String(data: data, encoding: .utf8) { try await task.send(.string(s)) }
        else { try await task.send(.data(data)) }
    }

    public func close() { task.cancel(with: .goingAway, reason: nil) }
}

public enum ConsoleKind: Sendable { case vnc, serial }

public extension MachinaClient {
    /// Opens a console WebSocket with a fresh single-use token. The `binary` subprotocol must be offered for VNC
    /// (the daemon echoes it; clients that offer none are dropped by browsers, and we mirror noVNC).
    func openConsole(_ vm: VmInfo, kind: ConsoleKind) async throws -> WebSocketTransport {
        let token = try await wsToken()
        var c = URLComponents(url: server.url, resolvingAgainstBaseURL: false)
        c?.scheme = server.url.scheme == "http" ? "ws" : "wss"
        var seg = CharacterSet.urlPathAllowed; seg.remove("/")
        c?.percentEncodedPath = (kind == .vnc ? "/ws/v1/vnc/" : "/ws/v1/console/") + (vm.name.addingPercentEncoding(withAllowedCharacters: seg) ?? vm.name)
        c?.queryItems = [URLQueryItem(name: "token", value: token)] + vm.scopeQuery
        guard let url = c?.url else { throw MachinaError.badURL }
        return WebSocketTransport(session: urlSession, url: url, protocols: kind == .vnc ? ["binary"] : [], text: kind == .serial)
    }
}
