import Testing
import Foundation
@testable import MachinaKit

private func row(_ o: [String: JSONValue]) -> ResourceRow { ResourceRow(id: "r", values: .object(o)) }
private func act(_ page: String, _ title: String) -> ResourceAction { FeatureRegistry.spec(page)!.actions.first { $0.title == title }! }

private struct Missing: Error {}
private func XCTUnwrapLike(_ e: Endpoint?) throws -> Endpoint { guard let e else { throw Missing() }; return e }

@Suite struct CloudOpsTests {
    @Test func pagesExistAndIdsUnique() {
        for id in ["fc-stack-ops", "fc-vpc-routing", "fc-routes", "fc-subnet-nat"] { #expect(FeatureRegistry.spec(id) != nil, "missing \(id)") }
        let ids = FeatureRegistry.all.map(\.id)
        #expect(Set(ids).count == ids.count)
    }

    @Test func driftIsReadOnlyAndNotAdminGated() throws {
        let a = act("fc-stack-ops", "Check drift")
        #expect(a.showsResult && !a.adminOnly)
        let e = try XCTUnwrapLike(a.endpoint(for: row(["id": .string("s 1")]), input: [:]))
        #expect(e.method == "GET" && e.path == "/api/v1/stacks/s%201/drift" && e.viaController)
    }

    @Test func convergeIsAdminTypedConfirmAndMentionsDrift() throws {
        let a = act("fc-stack-ops", "Converge…")
        #expect(a.adminOnly && a.destructive)
        let r = row(["id": .string("abc"), "name": .string("web")])
        #expect(a.phrase(for: r) == "web")
        #expect(a.confirm?.contains("Check drift") == true)
        let e = try XCTUnwrapLike(a.endpoint(for: r, input: [:]))
        #expect(e.method == "POST" && e.path == "/api/v1/stacks/abc/converge")
    }

    @Test func autoHealBodies() throws {
        let r = row(["id": .string("abc")])
        for (title, flag) in [("Auto-heal on", true), ("Auto-heal off", false)] {
            let a = act("fc-stack-ops", title)
            #expect(a.adminOnly)
            let e = try XCTUnwrapLike(a.endpoint(for: r, input: [:]))
            #expect(e.method == "PUT" && e.path == "/api/v1/stacks/abc/auto-heal" && e.body == .object(["enabled": .bool(flag)]))
        }
    }

    @Test func addRouteValidationAndBody() throws {
        let a = act("fc-vpc-routing", "Add route…")
        #expect(a.adminOnly)
        let r = row(["id": .string("vpc1")])
        #expect(throws: (any Error).self) { try a.endpoint(for: r, input: ["target": "local"]) }
        #expect(throws: (any Error).self) { try a.endpoint(for: r, input: ["destination": "10.0.0.0/8", "target": "nat"]) }
        let e = try XCTUnwrapLike(a.endpoint(for: r, input: ["destination": "10.60.0.0/16", "target": "blackhole"]))
        #expect(e.path == "/api/v1/cloud/vpcs/vpc1/routes")
        #expect(e.body == .object(["destination": .string("10.60.0.0/16"), "target": .string("blackhole")]))
    }

    @Test func removeRouteUsesVpcFromRow() throws {
        let a = act("fc-routes", "Remove route")
        #expect(a.adminOnly && a.destructive && a.confirm != nil)
        let e = try XCTUnwrapLike(a.endpoint(for: row(["id": .string("r9"), "vpc_id": .string("v1")]), input: [:]))
        #expect(e.method == "DELETE" && e.path == "/api/v1/cloud/vpcs/v1/routes/r9")
        #expect(a.endpoint(for: row(["id": .string("r9")]), inputValues: [:]) == nil)
    }

    @Test func natToggle() throws {
        let r = row(["id": .string("sn")])
        let on = try #require(act("fc-subnet-nat", "Enable NAT").endpoint(for: r, inputValues: [:]))
        let off = try #require(act("fc-subnet-nat", "Disable NAT").endpoint(for: r, inputValues: [:]))
        #expect(on.method == "PUT" && on.path == "/api/v1/cloud/subnets/sn/nat" && on.body == .object(["enabled": .bool(true)]))
        #expect(off.body == .object(["enabled": .bool(false)]))
        #expect(act("fc-subnet-nat", "Enable NAT").adminOnly && act("fc-subnet-nat", "Disable NAT").adminOnly)
    }
}
