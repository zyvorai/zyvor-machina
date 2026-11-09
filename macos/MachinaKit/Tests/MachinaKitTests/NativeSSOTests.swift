import Testing
import Foundation
@testable import MachinaKit

@Suite struct NativeSSOTests {
    @Test func pkceMatchesRFC7636Vector() {
        let p = PKCE(verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")
        #expect(p.challenge == "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM")
    }

    @Test func generatedPKCEIsWellFormedAndUnique() {
        let a = PKCE.generate(), b = PKCE.generate()
        #expect(a.verifier.count == 43 && a.challenge.count == 43)
        #expect(a.verifier != b.verifier)
        #expect(a.challenge == PKCE.challenge(for: a.verifier))
        let ok = Set("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_")
        #expect(a.verifier.allSatisfy { ok.contains($0) } && a.challenge.allSatisfy { ok.contains($0) })
    }

    @Test func startURL() throws {
        let u = try #require(NativeSSO.startURL(server: URL(string: "https://h.example:5092")!, challenge: "CH", state: "ST"))
        let c = try #require(URLComponents(url: u, resolvingAgainstBaseURL: false))
        #expect(c.host == "h.example" && c.port == 5092 && c.path == "/api/v1/auth/oidc/login")
        let q = Dictionary(uniqueKeysWithValues: (c.queryItems ?? []).map { ($0.name, $0.value ?? "") })
        #expect(q["native"] == "1" && q["redirect_uri"] == "machina://auth")
        #expect(q["code_challenge"] == "CH" && q["code_challenge_method"] == "S256" && q["state"] == "ST")
        let trailing = try #require(NativeSSO.startURL(server: URL(string: "https://h.example/")!, challenge: "C", state: "S"))
        #expect(trailing.path == "/api/v1/auth/oidc/login")
        #expect(NativeSSO.startURL(server: URL(string: "ftp://h.example")!, challenge: "C", state: "S") == nil)
    }

    @Test func acceptsGoodCallback() {
        let r = NativeSSO.parseCallback(URL(string: "machina://auth?code=abc123def&state=ST")!, expectedState: "ST")
        #expect(r == .success("abc123def"))
        #expect(NativeSSO.parseCallback(URL(string: "MACHINA://AUTH?code=ab&state=ST")!, expectedState: "ST") == .success("ab"))
    }

    @Test func rejectsWrongStateSchemeHostAndShape() {
        func fail(_ s: String, _ state: String = "ST") -> Bool {
            if case .failure = NativeSSO.parseCallback(URL(string: s)!, expectedState: state) { return true }
            return false
        }
        #expect(fail("machina://auth?code=ab&state=OTHER"))
        #expect(fail("machina://auth?code=ab"))
        #expect(fail("machina://auth?code=ab&state=ST&state=ST"))
        #expect(fail("machina://auth?code=ab&state=", ""))
        #expect(fail("https://auth?code=ab&state=ST"))
        #expect(fail("evil://auth?code=ab&state=ST"))
        #expect(fail("machina://evil?code=ab&state=ST"))
        #expect(fail("machina://auth.evil.com?code=ab&state=ST"))
        #expect(fail("machina://user@auth?code=ab&state=ST"))
        #expect(fail("machina://auth:99?code=ab&state=ST"))
        #expect(fail("machina://auth/extra?code=ab&state=ST"))
        #expect(fail("machina://auth?state=ST"))
        #expect(fail("machina://auth?code=&state=ST"))
        #expect(fail("machina://auth?code=a&code=b&state=ST"))
        #expect(fail("machina://auth?code=zz%20zz&state=ST"))
    }

    @Test func providerErrorIsSurfaced() {
        let r = NativeSSO.parseCallback(URL(string: "machina://auth?error=oidc&state=ST")!, expectedState: "ST")
        #expect(r == .failure(.providerError("oidc")))
    }

    @Test func providersReportNativeSupport() throws {
        let new = try #require(AuthProviders.decode(Data(#"{"native_sso":true,"pam":{"enabled":true},"oidc":{"enabled":true,"button_label":"Okta"},"saml":{"enabled":false}}"#.utf8)))
        #expect(new.canSignInWithSSO && !new.ssoServerTooOld)
        let old = try #require(AuthProviders.decode(Data(#"{"pam":{"enabled":true},"oidc":{"enabled":true},"saml":{"enabled":false}}"#.utf8)))
        #expect(!old.canSignInWithSSO && old.ssoServerTooOld)
        let none = try #require(AuthProviders.decode(Data(#"{"native_sso":true,"pam":{"enabled":true},"oidc":{"enabled":false}}"#.utf8)))
        #expect(!none.canSignInWithSSO && !none.ssoServerTooOld)
    }
}
