import Testing
import Foundation
@testable import MachinaKit

private func imClient() -> MachinaClient {
    let cfg = URLSessionConfiguration.ephemeral
    cfg.protocolClasses = [MockProtocol.self]
    return MachinaClient(server: ServerProfile(name: "t", url: URL(string: "https://h:5092")!), session: URLSession(configuration: cfg))
}
private let imRow = ResourceRow(id: "s-1", values: .object([
    "id": .string("s-1"), "hostname": .string("node7"), "bmc_address": .string("10.0.0.5")]))
private func imAct(_ spec: String, _ t: String) -> ResourceAction { FeatureRegistry.spec(spec)!.actions.first { $0.title == t }! }
private let imPC = "/api/v1/platform/controller"

extension ClientTests {
    @Test func imBootOverrideIsAdminTypedAndFollowedByPower() throws {
        let a = imAct("bm-servers", "Set boot device…")
        #expect(a.adminOnly && a.destructive && a.controller && a.phrase(for: imRow) == "node7")
        let vals = ["device": "cdrom", "persistent": "no", "power_step": "cycle"]
        let body = try a.inputBody(from: vals)
        let e = try #require(a.endpoint(for: imRow, inputValues: body))
        #expect(e.resolvedPath == "\(imPC)/api/v1/baremetal/servers/s-1/boot-device")
        #expect(e.body?["device"] == .string("cdrom") && e.body?["persistent"] == .bool(false))
        #expect(e.body?["power_step"] == nil)            // consumed by the follow-up, not sent to boot-device
        let f = try #require(a.followUpEndpoint(for: imRow, inputValues: body))
        #expect(f.method == "POST" && f.resolvedPath == "\(imPC)/api/v1/baremetal/servers/s-1/power" && f.body?["action"] == .string("cycle"))
        let none = try a.inputBody(from: vals.merging(["power_step": "none"]) { $1 })
        #expect(a.followUpEndpoint(for: imRow, inputValues: none) == nil)
        #expect(throws: ResourceCreate.Invalid.self) { try a.inputBody(from: vals.merging(["device": "floppy"]) { $1 }) }
        #expect(throws: ResourceCreate.Invalid.self) { try a.inputBody(from: vals.merging(["power_step": "off"]) { $1 }) }
    }

    @Test func imDeployOsIsAPickerOfStockPlusReadyImages() throws {
        let f = try #require(imAct("bm-servers", "Deploy image…").inputs.first { $0.id == "os" })
        let ch = try #require(f.choices)
        #expect(ch.endpoint.path == "/api/v1/baremetal/images")
        let json = JSONValue.object(["images": .array([
            .object(["name": .string("golden-1"), "status": .string("ready")]),
            .object(["name": .string("half"), "status": .string("downloading")]),
            .object(["name": .string("ubuntu-24.04"), "status": .string("ready")]),
        ])])
        let vals = ch.options(from: json).map(\.value)
        #expect(vals == BareMetalOps.stockOS + ["golden-1"])
        // A failed lookup still leaves the built-in installers.
        #expect(ch.options(from: .null).map(\.value) == BareMetalOps.stockOS)
    }

    @Test func imChoicesAreBackwardsCompatible() {
        let ch = ResourceCreate.Choices(.controllerGet("/x"), valueKeys: ["name"], labelKeys: ["name"])
        let json = JSONValue.array([.object(["name": .string("a")]), .object(["name": .string("b")])])
        #expect(ch.options(from: json).map(\.value) == ["a", "b"])
    }

    @Test func imImportValidatesAndNeedsAChecksumSource() throws {
        let c = BareMetalOps.importImage
        #expect(c.adminOnly && c.warning?.contains("checksum") == true)
        let sha = String(repeating: "a", count: 64)
        let ok = ["name": "golden-1", "url": "https://img.example/a.qcow2", "checksum_algo": "sha256", "checksum": sha]
        let body = try c.body(from: ok)
        #expect(body["name"] == .string("golden-1") && body["checksum"] == .string(sha))
        func bad(_ o: [String: String]) -> Bool { (try? c.body(from: o)) == nil }
        #expect(bad(ok.merging(["checksum": ""]) { $1 }))                                  // no checksum, no sums_url
        #expect(!bad(ok.merging(["checksum": "", "sums_url": "https://img.example/SHA256SUMS"]) { $1 }))
        #expect(bad(ok.merging(["url": "http://img.example/a.qcow2"]) { $1 }))             // https only
        #expect(bad(ok.merging(["url": "https://u:p@img.example/a.qcow2"]) { $1 }))
        #expect(bad(ok.merging(["sums_url": "https://u@img.example/SUMS"]) { $1 }))
        #expect(bad(ok.merging(["name": "Golden"]) { $1 }))
        #expect(bad(ok.merging(["name": ".hidden"]) { $1 }))
        #expect(bad(ok.merging(["checksum": "xyz"]) { $1 }))
        #expect(bad(ok.merging(["checksum_algo": "md5"]) { $1 }))
    }

    @Test func imDeleteAndCatalogImportActions() throws {
        let img = ResourceRow(id: "golden-1", values: .object(["name": .string("golden-1")]))
        let d = imAct("bm-images", "Delete image")
        #expect(d.method == "DELETE" && d.adminOnly && d.phrase(for: img) == "golden-1")
        #expect(d.endpoint(for: img)?.resolvedPath == "\(imPC)/api/v1/baremetal/images/golden-1")
        let cat = ResourceRow(id: "ubuntu-26.04", values: .object(["name": .string("ubuntu-26.04"), "os": .string("ubuntu"), "arch": .string("amd64")]))
        let i = imAct("bm-image-catalog", "Import")
        #expect(i.adminOnly && i.confirmText(for: cat)?.contains("ubuntu-26.04") == true)
        let e = try #require(i.endpoint(for: cat))
        #expect(e.method == "POST" && e.body?["name"] == .string("ubuntu-26.04") && e.body?["url"] == nil)
        #expect(FeatureRegistry.spec("bm-images")?.rowsPath == ["images"] && FeatureRegistry.spec("bm-image-catalog")?.rowsPath == ["catalog"])
    }

    @Test func imProvisionManyIsBoundedAndTyped() throws {
        let a = imAct("bm-profiles", "Provision machines…")
        let p = ResourceRow(id: "web", values: .object(["name": .string("web")]))
        #expect(a.adminOnly && a.phrase(for: p) == "web")
        let body = try a.inputBody(from: ["count": "3"])
        let e = try #require(a.endpoint(for: p, inputValues: body))
        #expect(e.body?["profile"] == .string("web") && e.body?["count"] == .number(3))
        #expect(throws: ResourceCreate.Invalid.self) { try a.inputBody(from: ["count": "0"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try a.inputBody(from: ["count": "101"]) }
    }

    @Test func imImageListDecodesThroughTheClient() async throws {
        MockProtocol.failWith = nil
        MockProtocol.handler = { _ in (200, #"{"images":[{"name":"g","status":"ready"}],"catalog":[{"name":"ubuntu-24.04"}]}"#) }
        let json = try await imClient().call(BareMetalOps.osChoices.endpoint)
        #expect(BareMetalOps.osChoices.options(from: json).map(\.value).last == "g")
    }
}
