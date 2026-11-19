import Testing
import Foundation
@testable import MachinaKit

// Vectors were made with a throwaway Ed25519 keypair (public half below); the production key is not involved.
private let vectorPublicKey = "TVWp6ZM0NuU-RBsOJ4lpWrfzT_G_5pWwvbMxeR6_tKk"
private let trialKey = "ZYV1.eyJ2IjoxLCJwcm9kdWN0IjoibWFjaGluYSIsImVkaXRpb24iOiJ0cmlhbCIsImlkIjoiMzZlZGUwM2JlYmQzN2Y5YSIsImxpY2Vuc2VlIjoiVGVzdCBMaWNlbnNlZSIsImVtYWlsIjoidGVzdEBleGFtcGxlLmNvbSIsIm1hY3MiOjEsImlzc3VlZCI6MTgwMDAwMDAwMCwiZXhwaXJlcyI6MTgwMjU5MjAwMH0.Kw1ptkEecvl47cmjnXALo9BOFQC49DVwjBIe296Qyci0l8vnnXeSjfP8NzfkFmjoUJiH17MZB37M9MF4_jTvDA"
private let proKey = "ZYV1.eyJ2IjoxLCJwcm9kdWN0IjoibWFjaGluYSIsImVkaXRpb24iOiJwcm8iLCJpZCI6ImEwZDQxYTYxNzYwN2I5ZWQiLCJsaWNlbnNlZSI6IlRlc3QgTGljZW5zZWUiLCJlbWFpbCI6InRlc3RAZXhhbXBsZS5jb20iLCJtYWNzIjozLCJpc3N1ZWQiOjE4MDAwMDAwMDAsImV4cGlyZXMiOjE4MzE1MzYwMDB9.dbZB7lQ3v0ecZawNDdV0Yltc-FMo9XkkgpT16uF30_hj6JTF2dl4gN2rjsxa1KqwYwV3CAgB_JpG5AwTDPvPAg"
private let expiredKey = "ZYV1.eyJ2IjoxLCJwcm9kdWN0IjoibWFjaGluYSIsImVkaXRpb24iOiJ0cmlhbCIsImlkIjoiMzllYWFmMjNhMjgxMDEwMCIsImxpY2Vuc2VlIjoiVGVzdCBMaWNlbnNlZSIsImVtYWlsIjoidGVzdEBleGFtcGxlLmNvbSIsIm1hY3MiOjEsImlzc3VlZCI6MTc5Njk3NjAwMCwiZXhwaXJlcyI6MTc5OTU2ODAwMH0.NF0oCL_LDfxn6bbZFEb12VUBhHxplRxHS7_T7SnWJeRpVEwR_Ejv5EI4AQb6Ettx7tVhESY50TfYUoQc4rT0CA"
private let wrongProductKey = "ZYV1.eyJ2IjoxLCJwcm9kdWN0IjoiZWx2b3JhIiwiZWRpdGlvbiI6InRyaWFsIiwiaWQiOiI5Y2RmNDIzZjM3MDY5OGJjIiwibGljZW5zZWUiOiJUZXN0IExpY2Vuc2VlIiwiZW1haWwiOiJ0ZXN0QGV4YW1wbGUuY29tIiwibWFjcyI6MSwiaXNzdWVkIjoxODAwMDAwMDAwLCJleHBpcmVzIjoxODAyNTkyMDAwfQ.K-JaEA6j3TII0KwzcxbPanloUlIXXmG69sQbQKjJIsNHypt2IQ385HBISSsUx7PWxYoO_Wk8HO7x6dEXcpZHCg"
private let issued = 1800000000.0

private func at(_ days: Double) -> Date { Date(timeIntervalSince1970: issued + days * 86400) }
private func verify(_ k: String, _ d: Double) -> LicenceStatus { LicenceKey.verify(k, publicKey: vectorPublicKey, now: at(d)) }
private func b64(_ d: Data) -> String { d.base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "") }
private func tampered(_ key: String) -> String {
    let parts = key.split(separator: ".").map(String.init)
    let body = Data(String(decoding: LicenceKey.decode(base64url: parts[1])!, as: UTF8.self).replacingOccurrences(of: "\"macs\":1", with: "\"macs\":9").utf8)
    return "ZYV1.\(b64(body)).\(parts[2])"
}
private func tmpFile() -> FileLicenceStorage {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("machina-licence-\(UUID().uuidString)")
    return FileLicenceStorage(url: dir.appendingPathComponent("licence.key"))
}

@Test func licenceValidTrialAndPro() {
    guard case let .valid(ed, who, email, macs, left, _) = verify(trialKey, 10) else { Issue.record("trial should verify"); return }
    #expect(ed == "trial" && who == "Test Licensee" && email == "test@example.com" && macs == 1 && left == 20)
    guard case let .valid(ed2, _, _, macs2, left2, _) = verify(proKey, 100) else { Issue.record("pro should verify"); return }
    #expect(ed2 == "pro" && macs2 == 3 && left2 == 265)
    #expect(verify(" \n" + trialKey + "\n", 1).isActive)   // pasted whitespace is ignored
}

@Test func licenceExpiryBoundary() {
    #expect(verify(trialKey, 29.5).isActive)
    #expect(!verify(trialKey, 31).isActive)
    guard case let .invalid(r) = verify(expiredKey, 0) else { Issue.record("expired key must be refused"); return }
    #expect(r.hasPrefix("expired"))
}

@Test func licenceRejectsTamperedWrongProductAndGarbage() {
    guard case let .invalid(r) = verify(tampered(trialKey), 1) else { Issue.record("tampered payload accepted"); return }
    #expect(r.hasPrefix("signature"))
    let parts = trialKey.split(separator: ".").map(String.init)
    var sig = Array(LicenceKey.decode(base64url: parts[2])!); sig[0] ^= 1
    #expect(!verify("ZYV1.\(parts[1]).\(b64(Data(sig)))", 1).isActive)
    guard case let .invalid(w) = verify(wrongProductKey, 0) else { Issue.record("wrong product accepted"); return }
    #expect(w.hasPrefix("wrong product"))
    for bad in ["", "garbage", "ZYV2.a.b", "ZYV1.only", "ZYV1..", "ZYV1.a b.c", "ZYV1.!!!.???", "ZYV1.\(parts[1]).\(parts[2]).extra"] {
        #expect(!verify(bad, 1).isActive, "should refuse \(bad.prefix(20))")
    }
}

@Test func licenceProductionKeyIsPinned() {
    #expect(LicenceKey.product == "machina")
    #expect(LicenceKey.decode(base64url: LicenceKey.publicKey)?.count == 32)
    #expect(!LicenceKey.verify(trialKey, now: at(1)).isActive)   // throwaway-keypair keys are not valid for the production key
    #expect(LicenceState.buyURL.absoluteString == "https://zyvor.dev/buy")
    #expect(LicenceState.trialURL.absoluteString.hasPrefix("mailto:sales@zyvor.dev"))
}

@MainActor @Test func licenceStatusTextPerState() {
    let file = tmpFile()
    func mgr(_ d: Double) -> LicenceManager { LicenceManager(storage: file, publicKey: vectorPublicKey, clock: { at(d) }) }
    var m = mgr(10)
    #expect(m.state == .unlicensed)
    #expect(m.state.statusText == "Unlicensed: free for evaluation and non-production use")
    #expect(m.state.showsBuyLink && m.state.showsTrialNote && !m.state.needsAttention)

    #expect(m.activate(trialKey) == nil)
    #expect(m.state.statusText == "Trial: 20 days left" && m.state.editionName == "Trial" && m.state.claims?.licensee == "Test Licensee")
    #expect(mgr(29.5).state.statusText == "Trial: 1 day left")
    #expect(mgr(31).state.statusText == "Licence expired on 14 Feb 2027")
    #expect(mgr(31).state.needsAttention && mgr(31).state.showsBuyLink && !mgr(31).state.showsTrialNote)

    m.remove()
    #expect(m.activate("  \n" + proKey + "\n") == nil)
    #expect(mgr(100).state.statusText == "Licensed, 265 days left")
    #expect(!mgr(100).state.needsAttention && !mgr(100).state.showsBuyLink)
    #expect(mgr(351).state.statusText == "Licensed, 14 days left" && mgr(351).state.needsAttention && mgr(351).state.showsBuyLink)
    #expect(!mgr(350).state.needsAttention)   // 15 days left
    #expect(mgr(366).state.statusText == "Licence expired on 15 Jan 2028")
    file.remove()
}

@MainActor @Test func licencePersistsAcrossRelaunchAndRemove() {
    let file = tmpFile()
    func mgr(_ d: Double) -> LicenceManager { LicenceManager(storage: file, publicKey: vectorPublicKey, clock: { at(d) }) }
    let m = mgr(10)
    #expect(m.activate(trialKey) == nil)
    #expect(file.load() == trialKey)
    let mode = (try? FileManager.default.attributesOfItem(atPath: file.url.path)[.posixPermissions] as? NSNumber)?.intValue
    #expect(mode == 0o600)
    #expect(mgr(12).state.statusText == "Trial: 18 days left")   // a new manager over the same storage = relaunch
    m.remove()
    #expect(m.state == .unlicensed && file.load() == nil && mgr(10).state == .unlicensed)
    file.remove()
}

@MainActor @Test func licenceRefusedKeysAreNeverSaved() {
    let file = tmpFile()
    let m = LicenceManager(storage: file, publicKey: vectorPublicKey, clock: { at(1) })
    #expect(m.activate(expiredKey) == "This key expired on 10 Jan 2027.")
    #expect(m.activate(tampered(trialKey))?.contains("signature") == true)
    #expect(m.activate(wrongProductKey)?.contains("wrong product") == true)
    #expect(m.activate("not a key") != nil && m.activate("   ") != nil)
    #expect(file.load() == nil && m.state == .unlicensed)
    _ = m.activate(trialKey); _ = m.activate(tampered(trialKey))
    #expect(file.load() == trialKey)   // a refused key leaves the saved one alone
    try? file.save(tampered(trialKey))   // altered on disk: a notice, not a lock-out
    let m2 = LicenceManager(storage: file, publicKey: vectorPublicKey, clock: { at(1) })
    guard case .unreadable = m2.state else { Issue.record("altered stored key should be unreadable"); return }
    #expect(m2.state.needsAttention)
    file.remove()
}

@Test func licenceVaultFallsBackToFile() {
    struct Refusing: LicenceStorage { func load() -> String? { nil }; func save(_ key: String) throws { throw LicenceError("no") }; func remove() {} }
    let file = tmpFile()
    let vault = LicenceVault(primary: Refusing(), fallback: file)
    try? vault.save("ZYV1.x.y")
    #expect(vault.load() == "ZYV1.x.y" && file.load() == "ZYV1.x.y")
    file.remove()
    // Keychain round trip under a test-only service name; tolerated when the Keychain is unavailable (e.g. headless).
    let kc = KeychainLicenceStorage(service: "dev.zyvor.machina.licence.test", account: "licence-key")
    kc.remove()
    if (try? kc.save("ZYV1.a.b")) != nil { #expect(kc.load() == "ZYV1.a.b"); kc.remove(); #expect(kc.load() == nil) }
}
