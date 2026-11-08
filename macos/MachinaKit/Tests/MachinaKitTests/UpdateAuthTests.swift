import Testing
import Foundation
@testable import MachinaKit

@Suite struct UpdateAuthTests {
    @Test func versionCompare() {
        #expect(AppVersion("v0.3.2")! > AppVersion("0.3.1")!)
        #expect(AppVersion("0.10.0")! > AppVersion("0.9.9")!)
        #expect(AppVersion("1.0")! == AppVersion("1.0.0")!)
        #expect(AppVersion("0.3.2-rc1")! == AppVersion("0.3.2")!)
        #expect(AppVersion("abc") == nil)
        #expect(AppVersion("") == nil)
    }
    @Test func releaseParseAndEvaluate() throws {
        let json = #"{"tag_name":"v0.4.0","html_url":"https://github.com/x/y/releases/tag/v0.4.0","assets":[{"name":"a.zip","browser_download_url":"https://e/a.zip"},{"name":"Machina-0.4.0.dmg","browser_download_url":"https://e/m.dmg"}]}"#
        let rel = try #require(UpdateChecker.parse(Data(json.utf8)))
        #expect(rel.version == "0.4.0")
        #expect(rel.dmgURL?.absoluteString == "https://e/m.dmg")
        #expect(UpdateChecker.evaluate(current: "0.1.0", release: rel) == .available(rel))
        #expect(UpdateChecker.evaluate(current: "0.4.0", release: rel) == .upToDate("0.4.0"))
        #expect(UpdateChecker.evaluate(current: "0.5.0", release: rel) == .upToDate("0.5.0"))
        #expect(UpdateChecker.parse(Data(#"{"tag_name":"v9.0.0","prerelease":true,"html_url":"https://x"}"#.utf8)) == nil)
        #expect(UpdateChecker.parse(Data("nope".utf8)) == nil)
    }
    @Test func providers() throws {
        let j = #"{"pam":{"enabled":true},"ldap":{"enabled":false},"oidc":{"enabled":true,"button_label":"Okta"},"saml":{"enabled":true,"button_label":"","login_available":false}}"#
        let p = try #require(AuthProviders.decode(Data(j.utf8)))
        #expect(p.pam && p.oidc && p.hasSSO && !p.ssoOnly)
        #expect(p.oidcLabel == "Okta" && p.samlLabel == nil && !p.samlLoginAvailable)
        let only = try #require(AuthProviders.decode(Data(#"{"pam":{"enabled":false},"oidc":{"enabled":true}}"#.utf8)))
        #expect(only.ssoOnly)
        #expect(AuthProviders.decode(Data("x".utf8)) == nil)
    }

    @Test func updateCheckReadsOnlyMacReleases() throws {
        let json = """
        [{"tag_name":"v0.3.2","html_url":"https://x/p","assets":[]},
         {"tag_name":"macos-v0.1.0","html_url":"https://x/a","assets":[]},
         {"tag_name":"macos-v0.10.0","html_url":"https://x/b","assets":[{"name":"Machina-0.10.0.dmg","browser_download_url":"https://x/b.dmg"}]},
         {"tag_name":"macos-v0.11.0","draft":true,"html_url":"https://x/c","assets":[]},
         {"tag_name":"macos-v0.12.0","prerelease":true,"html_url":"https://x/d","assets":[]}]
        """
        let rel = try #require(UpdateChecker.parseList(Data(json.utf8)))
        #expect(rel.version == "0.10.0" && rel.dmgURL?.absoluteString == "https://x/b.dmg")
        #expect(UpdateChecker.evaluate(current: "0.2.0", release: rel) == .available(rel))
        #expect(UpdateChecker.parseList(Data(#"[{"tag_name":"v0.3.2","html_url":"https://x"}]"#.utf8)) == nil)
    }
}
