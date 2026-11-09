import Foundation

/// Sign-in methods a daemon advertises at `GET /api/v1/auth/providers`.
public struct AuthProviders: Equatable, Sendable {
    public var pam = true
    public var ldap = false
    public var oidc = false
    public var oidcLabel: String?
    public var saml = false
    public var samlLabel: String?
    /// Server-side flag: the daemon has no SAML login flow yet (it always reports false).
    public var samlLoginAvailable = false
    /// Server-side flag (`native_sso`): the daemon supports the `machina://auth` code exchange (issue #224).
    public var nativeSSO = false

    public init() {}

    /// True when the server offers only SSO. OIDC works through the native code exchange on servers
    /// that report `native_sso`; SAML has no login flow in the daemon yet.
    public var ssoOnly: Bool { (oidc || saml) && !pam && !ldap }
    public var hasSSO: Bool { oidc || saml }
    /// OIDC is on and the server can hand the app a session through `machina://auth`.
    public var canSignInWithSSO: Bool { oidc && nativeSSO }
    /// OIDC is on but the daemon predates the native exchange endpoint.
    public var ssoServerTooOld: Bool { oidc && !nativeSSO }

    public static func decode(_ data: Data) -> AuthProviders? {
        guard let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        func on(_ k: String) -> Bool { (obj[k] as? [String: Any])?["enabled"] as? Bool ?? false }
        func label(_ k: String) -> String? {
            let s = (obj[k] as? [String: Any])?["button_label"] as? String
            return (s?.isEmpty ?? true) ? nil : s
        }
        var p = AuthProviders()
        p.pam = obj["pam"] == nil ? true : on("pam")
        p.ldap = on("ldap"); p.oidc = on("oidc"); p.saml = on("saml")
        p.oidcLabel = label("oidc"); p.samlLabel = label("saml")
        p.nativeSSO = obj["native_sso"] as? Bool ?? false
        p.samlLoginAvailable = (obj["saml"] as? [String: Any])?["login_available"] as? Bool ?? false
        return p
    }
}

extension MachinaClient {
    /// Public endpoint (no session needed). Returns nil when it fails or is unparseable.
    public func authProviders() async -> AuthProviders? {
        guard let data = try? await send("GET", "/api/v1/auth/providers", retry: false) else { return nil }
        return AuthProviders.decode(data)
    }
}
