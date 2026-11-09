import Foundation
import CryptoKit
import Security

/// Why a native SSO attempt failed.
public enum NativeSSOError: Error, LocalizedError, Equatable, Sendable {
    case serverTooOld, badServerURL, badCallback(String), providerError(String), exchangeRejected, cancelled
    public var errorDescription: String? {
        switch self {
        case .serverTooOld: return "This server is too old for single sign-on from Machina for Mac. Update the daemon, or use a local account."
        case .badServerURL: return "Enter a full server URL, e.g. https://host:5092"
        case .badCallback(let why): return "The sign-in response was not accepted (\(why))."
        case .providerError(let e): return "The identity provider reported an error (\(e))."
        case .exchangeRejected: return "The server rejected the sign-in code. It may have expired; try again."
        case .cancelled: return "Sign-in was cancelled."
        }
    }
}

/// PKCE (RFC 7636, S256) verifier and challenge.
public struct PKCE: Equatable, Sendable {
    public let verifier: String
    public let challenge: String

    public init(verifier: String) {
        self.verifier = verifier
        self.challenge = PKCE.challenge(for: verifier)
    }

    /// 32 random bytes, base64url without padding: 43 characters.
    public static func generate() -> PKCE {
        PKCE(verifier: randomToken(bytes: 32))
    }

    public static func challenge(for verifier: String) -> String {
        base64URL(Data(SHA256.hash(data: Data(verifier.utf8))))
    }

    static func randomToken(bytes n: Int) -> String {
        var raw = [UInt8](repeating: 0, count: n)
        if SecRandomCopyBytes(kSecRandomDefault, n, &raw) != errSecSuccess {
            // Practically unreachable; fall back to the system generator rather than a constant.
            var g = SystemRandomNumberGenerator()
            for i in 0..<n { raw[i] = UInt8.random(in: 0...255, using: &g) }
        }
        return base64URL(Data(raw))
    }

    static func base64URL(_ data: Data) -> String {
        data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }
}

/// Pure helpers for the native SSO flow (daemon issue #224): build the start URL, validate the
/// `machina://auth?code=` callback. No networking and no UI, so it is all unit tested.
public enum NativeSSO {
    public static let callbackScheme = "machina"
    public static let callbackHost = "auth"
    public static let redirectURI = "machina://auth"

    /// `GET /api/v1/auth/oidc/login?native=1&redirect_uri=...&code_challenge=...&code_challenge_method=S256&state=...`
    public static func startURL(server: URL, challenge: String, state: String) -> URL? {
        guard var c = URLComponents(url: server, resolvingAgainstBaseURL: false),
              let scheme = c.scheme?.lowercased(), scheme == "https" || scheme == "http", c.host != nil else { return nil }
        let base = c.percentEncodedPath.hasSuffix("/") ? String(c.percentEncodedPath.dropLast()) : c.percentEncodedPath
        c.percentEncodedPath = base + "/api/v1/auth/oidc/login"
        c.queryItems = [
            URLQueryItem(name: "native", value: "1"),
            URLQueryItem(name: "redirect_uri", value: redirectURI),
            URLQueryItem(name: "code_challenge", value: challenge),
            URLQueryItem(name: "code_challenge_method", value: "S256"),
            URLQueryItem(name: "state", value: state),
        ]
        return c.url
    }

    /// Validates the URL the session returned and extracts the exchange code. Rejects any other
    /// scheme, host, port, credentials or path, a missing or different `state`, and an `error`.
    public static func parseCallback(_ url: URL, expectedState: String) -> Result<String, NativeSSOError> {
        guard url.scheme?.lowercased() == callbackScheme else { return .failure(.badCallback("scheme")) }
        guard let c = URLComponents(url: url, resolvingAgainstBaseURL: false) else { return .failure(.badCallback("url")) }
        guard c.host?.lowercased() == callbackHost, c.port == nil, c.user == nil, c.password == nil else {
            return .failure(.badCallback("host"))
        }
        guard c.path.isEmpty || c.path == "/" else { return .failure(.badCallback("path")) }
        let items = c.queryItems ?? []
        func values(_ name: String) -> [String] { items.filter { $0.name == name }.compactMap(\.value) }
        // Exactly one state, equal to ours (the daemon echoes it on success and on error).
        guard values("state") == [expectedState], !expectedState.isEmpty else { return .failure(.badCallback("state")) }
        if let e = values("error").first { return .failure(.providerError(String(e.prefix(60)))) }
        let codes = values("code")
        guard codes.count == 1, let code = codes.first, isPlausibleCode(code) else { return .failure(.badCallback("code")) }
        return .success(code)
    }

    static func isPlausibleCode(_ s: String) -> Bool {
        !s.isEmpty && s.utf8.count <= 256 && s.utf8.allSatisfy { ($0 >= 0x30 && $0 <= 0x39) || ($0 >= 0x61 && $0 <= 0x66) || ($0 >= 0x41 && $0 <= 0x46) || $0 == 0x2d || $0 == 0x5f }
    }

    public static func newState() -> String { PKCE.randomToken(bytes: 16) }
}

extension MachinaClient {
    /// `POST /api/v1/auth/native/exchange`. The Set-Cookie lands in this client's URLSession, so the
    /// session behaves exactly like a password login. A 404 or 405 means the daemon predates the endpoint.
    @discardableResult
    public func nativeExchange(code: String, verifier: String) async throws -> SessionInfo {
        let body = try JSONSerialization.data(withJSONObject: ["code": code, "code_verifier": verifier])
        let data: Data
        do {
            data = try await send("POST", "/api/v1/auth/native/exchange", body: body, retry: false)
        } catch MachinaError.http(let status, _) where status == 404 || status == 405 {
            throw NativeSSOError.serverTooOld
        } catch MachinaError.unauthenticated {
            throw NativeSSOError.exchangeRejected
        }
        return (try? JSONDecoder().decode(SessionInfo.self, from: data)) ?? SessionInfo(authenticated: true, username: nil, role: nil)
    }
}
