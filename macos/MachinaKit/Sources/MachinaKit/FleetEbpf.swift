import Foundation

/// Fleet-wide eBPF writes: the VMM guard, XDP shield, sched_ext and VM edge forms of `EbpfForms.swift`, aimed at ONE
/// chosen fleet host through the controller passthrough `POST /api/v1/zeus-security/hosts/{id}/bpf`
/// (`controller/src/api/zeus_security.rs: host_bpf_call`, admin only). The body is the bpfd request itself:
/// `{"op": "guard_configure" | "shield_configure" | "scx_configure", "config": {...}}` or
/// `{"op": "vm_edge_sync", "state": {...}}`, the same payloads the daemon routes take.
///
/// Safety rules (checked in `FleetEbpfTests`):
/// - every form is `adminOnly`, and the controller enforces it too;
/// - the host is a required picker filled from `GET /api/v1/hosts`; there is no "all hosts" choice and a value that is
///   not exactly one `uuid|hostname` is refused, so one action touches one host;
/// - every rule of the local forms is inherited unchanged: explicit leases, typed phrases, audit first, allow lists;
/// - the confirmation names the target host and lists what changes.
public enum FleetEbpf {
    public static let hostKey = "host"
    public static let path = "/api/v1/zeus-security/hosts/{host}/bpf"
    public static let separator: Character = "|"

    public static let hostField = ResourceCreate.Field(
        hostKey, "Host (exactly one)",
        choices: .init(.controllerGet("/api/v1/hosts"), valueKeys: ["id", "hostname"], valueJoin: String(separator), labelKeys: ["hostname", "state"]))

    /// `uuid|hostname` from the picker. Anything else (empty, `all`, `*`, a list) is refused.
    public static func parseHost(_ raw: String) throws -> (id: String, name: String) {
        let parts = raw.split(separator: separator, maxSplits: 1, omittingEmptySubsequences: false).map(String.init)
        guard parts.count == 2, UUID(uuidString: parts[0]) != nil, !parts[1].isEmpty,
              !parts[1].contains(","), !["all", "*"].contains(parts[1].lowercased()) else {
            throw ResourceCreate.Invalid(message: "Pick exactly one host. Fleet-wide (all hosts) changes are not allowed.")
        }
        return (parts[0], parts[1])
    }

    /// The bpfd request for `op` from the flat form values (the host key is dropped).
    static func wrap(op: String, _ flat: JSONValue) throws -> JSONValue {
        var o = flat.object ?? [:]
        _ = try parseHost(o.removeValue(forKey: hostKey)?.text ?? "")
        switch op {
        case "vm_edge_sync":
            let state: JSONValue = o["name"] == nil ? .object(["vms": .array([]), "policy": .array([])])
                                                    : try ResourceCreate.BodyShape.vmEdge.apply(.object(o))
            return .object(["op": .string(op), "state": state])
        case "shield_configure":
            return .object(["op": .string(op), "config": try ResourceCreate.BodyShape.shield.apply(.object(o))])
        default:
            return .object(["op": .string(op), "config": .object(o)])
        }
    }

    private static func list(_ inner: JSONValue, _ keys: [String]) -> [String] {
        keys.compactMap { k in
            guard let a = inner[k]?.array, !a.isEmpty else { return nil }
            return "\(k) = " + a.map(\.text).joined(separator: ", ")
        }
    }
    private static func scalars(_ inner: JSONValue, _ keys: [String], prefix: String = "") -> [String] {
        keys.compactMap { k in
            guard let v = inner[k], v != .null, !v.text.isEmpty else { return nil }
            return "\(prefix)\(k) = \(v.text)"
        }
    }

    /// One line per thing the request sets, for the confirmation.
    static func changeLines(op: String, body: JSONValue) -> [String] {
        let inner = body["config"] ?? body["state"] ?? .object([:])
        switch op {
        case "vm_edge_sync":
            let vms = inner["vms"]?.array ?? []
            if vms.isEmpty { return ["Clear the whole VM edge state (every VM, every rule)"] }
            return ["Replace the whole VM edge state with: " + vms.compactMap { $0["name"]?.text }.joined(separator: ", ")] +
                vms.flatMap { scalars($0, ["isolate_ingress", "isolate_egress", "egress_mbps", "ingress_mbps", "pps"]) }
        case "guard_configure":
            return scalars(inner, ["enabled", "mode"]) + scalars(inner, ["lease_secs"]) + scalars(inner, ["exec", "wx", "devices"], prefix: "watch ")
                + list(inner, ["allow_exec", "allow_devices", "vms"])
        case "scx_configure":
            return scalars(inner, ["enabled", "lease_secs", "latency_target_us"]) + list(inner, ["vms"])
        default:
            return scalars(inner, ["iface", "mode", "protect_all", "syn_pps", "udp_pps", "icmp_pps", "other_pps", "burst_secs"])
                + list(inner, ["protected", "allow", "deny"])
        }
    }

    private static func derive(_ base: ResourceCreate, op: String, title: String) -> ResourceCreate {
        base.with {
            $0.title = title; $0.path = path; $0.controller = true; $0.method = "POST"
            $0.fields = [hostField] + base.fields
            $0.shape = .fleet(op)
            $0.warning = "ONE FLEET HOST, through the controller. " + (base.warning ?? "")
        }
    }

    private static func off(_ title: String, op: String, fields: [ResourceCreate.Field] = [], fixed: [String: JSONValue], warning: String,
                            phrase: String? = nil) -> ResourceCreate {
        ResourceCreate(title, path, controller: true, fields: [hostField] + fields).with {
            $0.method = "POST"; $0.adminOnly = true; $0.fixed = fixed; $0.shape = .fleet(op); $0.typedPhrase = phrase
            $0.warning = "ONE FLEET HOST, through the controller. " + warning
        }
    }

    private static func hostPage(_ id: String, _ title: String, icon: String, columns: [ResourceColumn], creates: [ResourceCreate]) -> ResourceSpec {
        ResourceSpec(id, title, group: "Fleet security", icon: icon, endpoint: .controllerGet("/api/v1/zeus-security/native-dataplane"),
                     rowsPath: ["hosts"], idKey: "host_id", columns: [.init("Host", "hostname")] + columns,
                     create: creates.first, moreCreates: Array(creates.dropFirst()), refreshSecs: 15)
    }

    public static let specs: [ResourceSpec] = {
        let guardSpec = FeatureRegistry.vmmGuard, shield = FeatureRegistry.bpfShield, scx = FeatureRegistry.bpfScx, edge = FeatureRegistry.bpfVmEdge
        return [
            hostPage("fleet-bpf-guard", "Fleet VMM guard", icon: "lock.shield",
                     columns: [.init("Enabled", "guard.config.enabled"), .init("Mode", "guard.config.mode"), .init("BPF-LSM", "guard.lsm_active")],
                     creates: [derive(guardSpec.create!, op: "guard_configure", title: "Audit on a host…"),
                               derive(guardSpec.moreCreates[0], op: "guard_configure", title: "Enforce on a host…"),
                               off("Turn guard off on a host…", op: "guard_configure",
                                   fixed: ["enabled": .bool(false), "mode": .string("audit"), "lease_secs": .null],
                                   warning: "Disables the VMM guard on that host: no more audit records and no blocking of QEMU exec, W+X mappings or device opens.")]),
            hostPage("fleet-bpf-shield", "Fleet XDP shield", icon: "shield.checkered",
                     columns: [.init("Mode", "shield.mode"), .init("Interface", "shield.iface")],
                     creates: [derive(shield.create!, op: "shield_configure", title: "Audit on a host…"),
                               derive(shield.moreCreates[0], op: "shield_configure", title: "Enforce on a host (drops traffic)…"),
                               off("Turn shield off on a host…", op: "shield_configure",
                                   fields: [.init("iface", "Uplink interface it is attached to")], fixed: ["mode": .string("off")],
                                   warning: "Detaches the shield from that interface on that host: no more rate limiting or audit counters.")]),
            hostPage("fleet-bpf-scx", "Fleet VM scheduler", icon: "cpu",
                     columns: [.init("Enabled", "scx.enabled"), .init("State", "scx.state")],
                     creates: [derive(scx.create!, op: "scx_configure", title: "Start on a host under a lease…"),
                               off("Stop scheduler on a host…", op: "scx_configure", fixed: ["enabled": .bool(false)],
                                   warning: "Stops the helper on that host and moves the listed VMs' vCPU threads back to the fair scheduler class.")]),
            hostPage("fleet-bpf-vmedge", "Fleet VM edge", icon: "rectangle.connected.to.line.below",
                     columns: [.init("Sandbox", "vm_sandbox.mode")],
                     creates: [derive(edge.create!, op: "vm_edge_sync", title: "Set one VM's edge on a host (replaces state)…"),
                               off("Clear VM edge on a host…", op: "vm_edge_sync", fixed: [:],
                                   warning: "Removes every VM from the edge on that host, including entries the controller synced: rate limits and isolation stop. The controller may push its state again on its next sync.",
                                   phrase: "CLEAR VM EDGE")]),
        ]
    }()

    /// The four pages, for tests and audits.
    public static var writeSpecs: [ResourceSpec] { specs }
}

public extension ResourceCreate {
    /// True for a form that acts on one fleet host (path carries `{host}`).
    var isFleet: Bool { path.contains("{host}") }

    /// The request for the typed `values` (the picker value is `uuid|hostname`); other forms keep their fixed endpoint.
    func endpoint(values: [String: String]) throws -> Endpoint {
        guard isFleet else { return endpoint }
        let host = try FleetEbpf.parseHost(values[FleetEbpf.hostKey] ?? "")
        var e = endpoint
        e.path = path.replacingOccurrences(of: "{host}", with: MachinaClient.seg(host.id))
        return e
    }

    /// Text for the confirmation sheet: the warning, plus for a fleet form the target host and the list of changes.
    func confirmation(values: [String: String], body: JSONValue) -> String {
        let base = warning ?? "This changes the server."
        guard isFleet, case .fleet(let op)? = shape, let host = try? FleetEbpf.parseHost(values[FleetEbpf.hostKey] ?? "") else { return base }
        let lines = FleetEbpf.changeLines(op: op, body: body).map { "  - " + $0 }.joined(separator: "\n")
        return "Target host: \(host.name) (\(host.id)). No other host is touched.\n\nThis will set:\n\(lines)\n\n\(base)"
    }
}
