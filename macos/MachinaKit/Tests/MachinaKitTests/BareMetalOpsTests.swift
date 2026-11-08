import Testing
import Foundation
@testable import MachinaKit

private func bmClient(_ url: String = "https://h:5092") -> MachinaClient {
    let cfg = URLSessionConfiguration.ephemeral
    cfg.protocolClasses = [MockProtocol.self]
    return MachinaClient(server: ServerProfile(name: "t", url: URL(string: url)!), session: URLSession(configuration: cfg))
}
private func bmBody(_ req: URLRequest) -> String {
    if let b = req.httpBody { return String(decoding: b, as: UTF8.self) }
    guard let st = req.httpBodyStream else { return "" }
    st.open(); defer { st.close() }
    var d = Data(); var buf = [UInt8](repeating: 0, count: 1024)
    while st.hasBytesAvailable { let n = st.read(&buf, maxLength: 1024); if n <= 0 { break }; d.append(buf, count: n) }
    return String(decoding: d, as: UTF8.self)
}
private let srv = ResourceRow(id: "s-1", values: .object([
    "id": .string("s-1"), "hostname": .string("node7"), "bmc_address": .string("10.0.0.5"), "bmc_type": .string("ipmi"),
    "bmc_credentials_configured": .bool(true), "mac_address": .string("aa:bb:cc:dd:ee:ff"), "cpu_cores": .number(32), "memory_mib": .number(65536), "lifecycle": .string("allocated")]))
private func act(_ t: String) -> ResourceAction { FeatureRegistry.spec("bm-servers")!.actions.first { $0.title == t }! }
private let pc = "/api/v1/platform/controller"

extension ClientTests {
    @Test func bmEveryWriteIsAdminOnlyAndTypesTheHostname() {
        for t in ["Set BMC login…", "Rotate BMC password…", "Deploy image…", "Insert virtual media…", "Eject virtual media"] {
            let a = act(t)
            #expect(a.adminOnly && a.destructive && a.controller, Comment(rawValue: t))
            #expect(a.phrase(for: srv) == "node7", Comment(rawValue: t))
        }
        #expect(FeatureRegistry.spec("bm-servers")?.accessory == .bareMetalSol)
    }

    @Test func bmRotatePasswordPayloadKeepsExactCharactersAndPrefillsRow() throws {
        let a = act("Rotate BMC password…")
        let init0 = a.initialValues(for: srv)
        #expect(init0["bmc_type"] == "ipmi" && init0["bmc_address"] == "10.0.0.5" && init0["bmc_password"] == "")
        let body = try a.inputBody(from: ["bmc_type": "ipmi", "bmc_address": "10.0.0.5", "bmc_username": "admin", "bmc_password": " p@ss w0rd "])
        #expect(body["bmc_password"] == .string(" p@ss w0rd "))
        #expect(body["boot_method"] == nil)
        #expect(throws: ResourceCreate.Invalid.self) { try a.inputBody(from: ["bmc_type": "ipmi", "bmc_address": "10.0.0.5", "bmc_username": "admin"]) }
        let e = try #require(a.endpoint(for: srv, inputValues: body))
        #expect(e.method == "PUT" && e.resolvedPath == "\(pc)/api/v1/baremetal/servers/s-1/bmc")
    }

    @Test func bmSetLoginMayKeepStoredPassword() throws {
        let body = try act("Set BMC login…").inputBody(from: ["bmc_type": "redfish", "bmc_address": "https://bmc.lan", "bmc_username": "root"])
        #expect(body["bmc_password"] == nil)
    }

    @Test func bmInputValidation() {
        let a = act("Set BMC login…")
        func bad(_ o: [String: String]) -> Bool { (try? a.inputBody(from: o)) == nil }
        let ok = ["bmc_type": "redfish", "bmc_address": "10.0.0.5:443", "bmc_username": "root", "bmc_password": "x"]
        #expect(!bad(ok))
        #expect(bad(ok.merging(["bmc_type": "libvirt"]) { $1 }))
        #expect(bad(ok.merging(["bmc_address": "root:pw@10.0.0.5"]) { $1 }))
        #expect(bad(ok.merging(["bmc_address": "10.0.0.5; rm -rf /"]) { $1 }))
        #expect(bad(ok.merging(["bmc_address": "ftp://x"]) { $1 }))
        #expect(bad(ok.merging(["bmc_username": "a b"]) { $1 }))
        #expect(bad(ok.merging(["bmc_password": "line\nbreak"]) { $1 }))
        #expect(bad(ok.merging(["boot_method": "floppy"]) { $1 }))
    }

    @Test func bmDeployPayloadListsOverwritesAndNamesTheMachine() throws {
        let a = act("Deploy image…")
        let body = try a.inputBody(from: ["os": "ubuntu-24.04", "ssh_authorized_keys": "ssh-ed25519 AAAA me@x\n\nssh-rsa BBBB you", "timezone": "Europe/Berlin", "ttl_hours": "2"])
        #expect(body["ssh_authorized_keys"] == .array([.string("ssh-ed25519 AAAA me@x"), .string("ssh-rsa BBBB you")]))
        #expect(body["ttl_hours"] == .number(2) && body["timezone"] == .string("Europe/Berlin"))
        #expect(throws: ResourceCreate.Invalid.self) { try a.inputBody(from: ["os": "ubuntu 24; x"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try a.inputBody(from: ["os": "rocky-9", "timezone": "../etc"]) }
        let text = try #require(a.confirmText(for: srv))
        #expect(text.contains("node7") && text.contains("aa:bb:cc:dd:ee:ff") && text.contains("65536") && text.contains("overwrite") && text.contains("every disk"))
        #expect(!text.contains("{"))
        let e = try #require(a.endpoint(for: srv, inputValues: body))
        #expect(e.method == "POST" && e.resolvedPath == "\(pc)/api/v1/baremetal/servers/s-1/deploy")
    }

    @Test func bmVirtualMediaInsertEjectAndUrlRules() throws {
        let ins = act("Insert virtual media…")
        let e = try #require(ins.endpoint(for: srv, inputValues: try ins.inputBody(from: ["url": "https://repo.lan/a.iso"])))
        #expect(e.resolvedPath.hasSuffix("/servers/s-1/virtual-media") && e.body == .object(["action": .string("insert"), "url": .string("https://repo.lan/a.iso")]))
        #expect((try? ins.inputBody(from: [:]))?.isEmpty == true)
        for bad in ["ftp://x/a.iso", "https://u:p@h/a.iso", "file:///etc/passwd", "not a url"] {
            #expect(throws: ResourceCreate.Invalid.self) { try ins.inputBody(from: ["url": bad]) }
        }
        #expect(try #require(ins.confirmText(for: srv)).contains("overwrite"))
        let ej = try #require(act("Eject virtual media").endpoint(for: srv))
        #expect(ej.body == .object(["action": .string("eject")]))
    }

    @Test func bmSecretsAreRedactedFromCurlAndNeverTravelOverPlainHttp() async throws {
        let e = Endpoint("PUT", "/api/v1/baremetal/servers/s-1/bmc", body: .object(["bmc_username": .string("root"), "bmc_password": .string("hunter2")]), controller: true)
        let cmd = Curl.script(e, server: URL(string: "https://h:5092")!)
        #expect(!cmd.contains("hunter2") && cmd.contains("<redacted>") && cmd.contains("root"))
        #expect(SecretRedaction.redact(.object(["nested": .object(["api_key": .string("k")]), "list": .array([.object(["token": .string("t")])])]))
                == .object(["nested": .object(["api_key": .string("<redacted>")]), "list": .array([.object(["token": .string("<redacted>")])])]))
        MockProtocol.failWith = nil
        let hit = Counter()
        MockProtocol.handler = { _ in hit.add("x"); return (200, "{}") }
        await #expect(throws: BareMetalError.plainHTTP) { try await bmClient("http://h:5092").call(e) }
        #expect(hit.count("x") == 0)
        _ = try await bmClient().call(e)
        #expect(hit.count("x") == 1)
    }

    @Test func bmPasswordGoesOnTheWireOnlyInTheBody() async throws {
        let seen = Counter()
        MockProtocol.failWith = nil
        MockProtocol.handler = { req in seen.add(req.url!.absoluteString); seen.add("BODY " + bmBody(req)); return (200, #"{"id":"s-1","bmc_type":"ipmi"}"#) }
        let a = act("Rotate BMC password…")
        let body = try a.inputBody(from: ["bmc_type": "ipmi", "bmc_address": "10.0.0.5", "bmc_username": "admin", "bmc_password": "s3cret!"])
        _ = try await bmClient().call(try #require(a.endpoint(for: srv, inputValues: body)))
        let all = seen.all()
        #expect(all.filter { $0.contains("s3cret!") }.count == 1 && all.first { $0.contains("s3cret!") }?.hasPrefix("BODY") == true)
        #expect(try #require(a.confirmText(for: srv)).contains("s3cret!") == false)
    }

    @Test func bmSolTokenThenUrlIsValidated() async throws {
        MockProtocol.failWith = nil
        MockProtocol.handler = { req in
            (req.httpMethod == "POST" && req.url!.path == "\(pc)/api/v1/baremetal/servers/s-1/sol-token") ? (200, #"{"ws_path":"/ws/v1/baremetal/s-1/sol?token=abc123"}"#) : (500, "")
        }
        let u = try await bmClient().solURL(serverId: "s-1")
        #expect(u.absoluteString == "wss://h:5093/ws/v1/baremetal/s-1/sol?token=abc123")
        MockProtocol.handler = { _ in (200, #"{"ws_path":"/ws/v1/baremetal/OTHER/sol?token=abc"}"#) }
        await #expect(throws: BareMetalError.badSolPath) { try await bmClient().solURL(serverId: "s-1") }
        MockProtocol.handler = { _ in (403, "admin required") }
        await #expect(throws: MachinaError.self) { try await bmClient().solURL(serverId: "s-1") }
        for bad in ["/ws/v1/baremetal/s-1/sol", "/ws/v1/baremetal/s-1/sol?token=", "/ws/v1/baremetal/s-1/sol?token=a&x=//evil", "//evil/ws?token=a"] {
            #expect(throws: BareMetalError.badSolPath) { try BareMetalOps.solURL(wsPath: bad, serverId: "s-1", server: URL(string: "https://h:5092")!) }
        }
    }

    @Test func bmSolNeedsIpmiWithStoredLogin() {
        #expect(BareMetalOps.solBlocker(row: srv) == nil)
        let redfish = ResourceRow(id: "x", values: .object(["bmc_type": .string("redfish"), "bmc_credentials_configured": .bool(true)]))
        let nologin = ResourceRow(id: "x", values: .object(["bmc_type": .string("ipmi"), "bmc_credentials_configured": .bool(false)]))
        #expect(BareMetalOps.solBlocker(row: redfish) != nil && BareMetalOps.solBlocker(row: nologin) != nil)
    }

    @Test func bmPowerStaysAnExplicitTargetState() throws {
        let on = try #require(act("Power on").endpoint(for: srv))
        #expect(on.body == .object(["action": .string("on")]) && on.method == "POST")
    }
}
