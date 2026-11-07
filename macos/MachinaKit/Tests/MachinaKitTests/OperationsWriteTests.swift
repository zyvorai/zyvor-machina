import Testing
import Foundation
@testable import MachinaKit

@Suite(.serialized) struct OperationsWriteTests {
    private func spec(_ id: String) -> ResourceSpec { FeatureRegistry.spec(id)! }
    private func row(_ json: String) -> ResourceRow { ResourceRow(id: "r", values: try! JSONDecoder().decode(JSONValue.self, from: Data(json.utf8))) }
    private func bodyJSON(_ e: Endpoint) -> [String: JSONValue] { e.body?.object ?? [:] }
    private let empty = ResourceRow(id: "", values: .object([:]))

    @Test func scaleDeploymentRequest() throws {
        let a = spec("k8s-deployments").actions.first { $0.title == "Scale" }!
        let r = row(#"{"metadata":{"name":"web","namespace":"prod"}}"#)
        let e = a.endpoint(for: r, inputValues: try a.inputBody(from: ["replicas": "3"]))!
        #expect(e.method == "POST"); #expect(e.resolvedPath == "/api/v1/k8s/action"); #expect(!e.viaController)
        let b = bodyJSON(e)
        #expect(b["action"] == .string("scale_deployment")); #expect(b["name"] == .string("web"))
        #expect(b["namespace"] == .string("prod")); #expect(b["replicas"] == .number(3))
        #expect(a.confirm != nil)
    }

    @Test func scaleRejectsBadReplicas() {
        let a = spec("k8s-deployments").actions.first { $0.title == "Scale" }!
        #expect(throws: ResourceCreate.Invalid.self) { try a.inputBody(from: ["replicas": "-1"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try a.inputBody(from: ["replicas": "x"]) }
    }

    @Test func restartAndDeletePod() {
        let r = row(#"{"metadata":{"name":"p-1","namespace":"kube-system"}}"#)
        let restart = spec("k8s-deployments").actions.first { $0.title == "Restart" }!.endpoint(for: r)!
        #expect(bodyJSON(restart)["action"] == .string("rollout_restart_deployment"))
        let del = spec("k8s-pods").actions.first { $0.title == "Delete pod" }!
        #expect(del.destructive)
        let e = del.endpoint(for: r)!
        #expect(bodyJSON(e)["action"] == .string("delete_pod")); #expect(bodyJSON(e)["name"] == .string("p-1")); #expect(bodyJSON(e)["namespace"] == .string("kube-system"))
    }

    @Test func kubectlFailureIsSurfaced() {
        let bad = JSONValue.object(["ok": .bool(false), "stderr": .string("not found\n"), "exit_code": .number(1)])
        #expect(ResourceAction.failure(in: bad) == "not found")
        #expect(ResourceAction.failure(in: .object(["ok": .bool(true)])) == nil)
    }

    @Test func drsDryRunAndExecute() throws {
        let acts = spec("drs-rebalance").actions
        let dry = acts.first { $0.title == "Dry run" }!
        let e = dry.endpoint(for: empty, inputValues: try dry.inputBody(from: ["max_moves": "2"]))!
        #expect(e.resolvedPath == "/api/v1/platform/controller/api/v1/ai/fleet/rebalance/execute")
        #expect(bodyJSON(e)["dry_run"] == .bool(true)); #expect(bodyJSON(e)["max_moves"] == .number(2))
        let ex = acts.first { $0.title == "Execute moves" }!
        #expect(ex.destructive && ex.confirm != nil && ex.global)
        #expect(bodyJSON(ex.endpoint(for: empty, inputValues: ["max_moves": .number(1)])!)["dry_run"] == .bool(false))
        let prop = spec("drs").actions.first!
        #expect(prop.endpoint(for: empty)!.resolvedPath.hasSuffix("/drs/consolidation/propose"))
    }

    @Test func haPolicyRequest() {
        let r = row(#"{"id":"11111111-2222-3333-4444-555555555555","name":"x"}"#)
        let on = spec("fc-instances").actions.first { $0.title == "Enable HA" }!.endpoint(for: r)!
        #expect(on.resolvedPath == "/api/v1/platform/controller/api/v1/vms/11111111-2222-3333-4444-555555555555/ha")
        #expect(bodyJSON(on)["enabled"] == .bool(true))
        let off = spec("fc-instances").actions.first { $0.title == "Disable HA" }!.endpoint(for: r)!
        #expect(bodyJSON(off)["enabled"] == .bool(false))
    }

    @Test func createInstanceBodyAndChoices() throws {
        let c = spec("fc-instances").create!
        #expect(c.path == "/api/v1/vms/from-template")
        let body = try c.body(from: ["name": "tmp-x", "template_ref": "ubuntu@1.0.0", "flavor_id": "f1", "network": "", "key_name": ""])
        #expect(body.object == ["name": .string("tmp-x"), "template_ref": .string("ubuntu@1.0.0"), "flavor_id": .string("f1")])
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["name": "a"]) }
        let ch = c.fields.first { $0.id == "template_ref" }!.choices!
        let json = try JSONDecoder().decode(JSONValue.self, from: Data(#"[{"name":"ubuntu","version":"1.0.0","os_family":"linux"},{"name":"bad"}]"#.utf8))
        let opts = ch.options(from: json)
        #expect(opts.count == 1); #expect(opts[0].value == "ubuntu@1.0.0"); #expect(opts[0].label == "ubuntu · 1.0.0 · linux")
    }
}
