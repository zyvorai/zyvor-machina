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

    public init() {}

    /// True when the server offers browser SSO that this app cannot complete.
    /// The daemon's OIDC callback sets the HttpOnly `machina_session` cookie on its own https
    /// origin and redirects to `/`; it never redirects to a custom URL scheme, so an
    /// ASWebAuthenticationSession cannot receive anything and the cookie is not readable by the app.
    public var ssoOnly: Bool { (oidc || saml) && !pam && !ldap }
    public var hasSSO: Bool { oidc || saml }

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
