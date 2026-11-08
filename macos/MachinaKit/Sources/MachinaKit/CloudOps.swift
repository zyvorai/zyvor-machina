import Foundation

/// Fleet Cloud operations pages: stack drift / converge / auto-heal, VPC routes and subnet NAT.
/// Paths and bodies were checked against `controller/src/api/stacks.rs` (`get_drift`, `converge_stack`, `set_auto_heal`)
/// and `controller/src/api/cloud/network.rs` (`create_route`, `delete_route`, `plan`, `set_subnet_nat`).
///
/// Not here, and why: the native load balancer has exactly one listener (`listener_port` + `protocol` on the balancer
/// row, no listener routes), so "add/edit/remove listeners" does not exist natively; members and weights are already on
/// the Load balancers pages. Multiple listeners exist only on the SigV4 ELBv2 API, which a JWT client cannot call.
/// Routes belong to the VPC (there are no separate route tables), so there is no subnet association call either.
extension FeatureRegistry {
    static let cloudOpsSpecs: [ResourceSpec] = [
        ResourceSpec("fc-stack-ops", "Stack drift and heal", group: "Fleet Cloud", icon: "arrow.triangle.2.circlepath", endpoint: .controllerGet("/api/v1/stacks"), idKey: "id",
                     columns: [.init("Name", "name"), .init("Status", "status"), .init("In sync", "drift_json.in_sync"), .init("Open drift", "drift_json.open"),
                               .init("Auto-heal", "auto_heal"), .init("Checked", "checked_at", .date)],
                     actions: [
                        ResourceAction("Check drift", "GET", "/api/v1/stacks/{id}/drift", controller: true, showsResult: true),
                        ResourceAction("Converge…", "/api/v1/stacks/{id}/converge", destructive: true, controller: true)
                            .with {
                                $0.adminOnly = true; $0.typeToConfirm = "name"; $0.timeout = 120
                                $0.confirm = "Recreates what is missing or changed so the stack matches its template again. Run Check drift first and read what it lists: every open item may create, change or restart resources."
                            },
                        ResourceAction("Auto-heal on", "PUT", "/api/v1/stacks/{id}/auto-heal", controller: true, staticBody: .object(["enabled": .bool(true)]))
                            .with { $0.adminOnly = true; $0.confirm = "The controller will converge this stack by itself whenever it finds drift." },
                        ResourceAction("Auto-heal off", "PUT", "/api/v1/stacks/{id}/auto-heal", controller: true, staticBody: .object(["enabled": .bool(false)]))
                            .with { $0.adminOnly = true; $0.confirm = "Drift will only be reported; nothing is fixed until you converge." },
                     ]),
        ResourceSpec("fc-vpc-routing", "VPC routing", group: "Fleet Cloud", icon: "point.topleft.down.curvedto.point.bottomright.up", endpoint: .controllerGet("/api/v1/project-registry"),
                     columns: [.init("Project", "_parent"), .init("VPC", "name"), .init("CIDR", "cidr"), .init("ID", "id")],
                     actions: [
                        ResourceAction("Preview plan", "GET", "/api/v1/cloud/vpcs/{id}/plan", controller: true, showsResult: true),
                        ResourceAction("Add route…", "/api/v1/cloud/vpcs/{id}/routes", controller: true,
                                       inputs: [.init("destination", "Destination CIDR (e.g. 10.60.0.0/16)"),
                                                .init("target", "Target", .choice(["local", "blackhole", "peering"]), initial: "local"),
                                                .init("target_id", "Peering id (only for peering)", required: false)])
                            .with {
                                $0.adminOnly = true
                                $0.confirm = "Adds this route to the VPC and pushes forwarding to the host now. A blackhole route drops that traffic; check Preview plan first."
                            },
                     ],
                     fanOut: .init(parent: .controllerGet("/api/v1/project-registry"), childPath: "/api/v1/cloud/projects/{id}/vpcs")),
        ResourceSpec("fc-routes", "VPC routes", group: "Fleet Cloud", icon: "arrow.triangle.turn.up.right.diamond", endpoint: .controllerGet("/api/v1/project-registry"),
                     columns: [.init("VPC", "_parent"), .init("Destination", "destination"), .init("Target", "target"), .init("Target id", "target_id"), .init("ID", "id")],
                     actions: [
                        ResourceAction("Remove route", "DELETE", "/api/v1/cloud/vpcs/{vpc_id}/routes/{id}", destructive: true, controller: true)
                            .with {
                                $0.adminOnly = true
                                $0.confirm = "Removes this route and withdraws its forwarding from the host now. Traffic to the destination follows the remaining routes."
                            },
                     ],
                     fanOut: .init(parent: .controllerGet("/api/v1/project-registry"), childPath: "/api/v1/cloud/vpcs/{id}/routes",
                                   intermediatePath: "/api/v1/cloud/projects/{id}/vpcs")),
        ResourceSpec("fc-subnet-nat", "Subnet NAT", group: "Fleet Cloud", icon: "arrow.up.forward.app", endpoint: .controllerGet("/api/v1/project-registry"),
                     columns: [.init("VPC", "_parent"), .init("Subnet", "name"), .init("CIDR", "cidr"), .init("Status", "status"), .init("NAT", "nat_enabled"), .init("ID", "id")],
                     actions: [
                        ResourceAction("Enable NAT", "PUT", "/api/v1/cloud/subnets/{id}/nat", controller: true, staticBody: .object(["enabled": .bool(true)]))
                            .with {
                                $0.adminOnly = true
                                $0.confirm = "Instances in this subnet will reach the outside masqueraded behind the host's uplink address. The subnet must be ready."
                            },
                        ResourceAction("Disable NAT", "PUT", "/api/v1/cloud/subnets/{id}/nat", destructive: true, controller: true, staticBody: .object(["enabled": .bool(false)]))
                            .with {
                                $0.adminOnly = true
                                $0.confirm = "Instances in this subnet lose outbound internet access through the host."
                            },
                     ],
                     fanOut: .init(parent: .controllerGet("/api/v1/project-registry"), childPath: "/api/v1/cloud/vpcs/{id}/subnets",
                                   intermediatePath: "/api/v1/cloud/projects/{id}/vpcs")),
    ]
}
