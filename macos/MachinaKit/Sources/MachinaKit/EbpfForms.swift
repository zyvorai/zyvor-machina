import Foundation

/// Write forms for four eBPF features that were read-only: VMM guard, XDP DDoS shield, sched_ext VM scheduler and the
/// VM edge. Paths and payloads follow `daemon/src/routes/bpf.rs` and `web/src/api/bpf.ts`; every write is admin-only.
///
/// Safety rules enforced here (and in `EbpfFormsTests`):
/// - every form and action is `adminOnly`;
/// - audit / dry-run is what each page offers first; enforcing forms ask for an explicit lease (guard 1...3600 s,
///   scx 1...3600 s, never the server default) and a typed phrase;
/// - the shield refuses to enforce without an explicit allow list (your own address), a protected target and the phrase;
/// - the VM edge form replaces the whole edge state, so it needs a typed phrase and defaults to no isolation.
extension FeatureRegistry {
    private static func obj(_ pairs: [String: JSONValue]) -> JSONValue { .object(pairs) }
    static let guardLeaseRange = 1...3600
    static let scxLeaseRange = 1...3600

    static let ebpfFormSpecs: [ResourceSpec] = [bpfShield, bpfScx, bpfVmEdge]

    // MARK: VMM guard (`PUT /api/v1/bpf/guard`)

    private static func guardFields(enforce: Bool) -> [ResourceCreate.Field] {
        var f: [ResourceCreate.Field] = []
        if enforce { f.append(.init("lease_secs", "Lease (seconds, 1 to 3600)", .int, initial: "300", range: guardLeaseRange)) }
        f += [
            .init("exec", "Watch exec outside QEMU", .bool, initial: "yes", required: false),
            .init("wx", "Watch W+X mappings", .bool, initial: "yes", required: false),
            .init("devices", "Watch device opens", .bool, initial: "yes", required: false),
            .init("allow_exec", "Extra allowed executables", .list, required: false),
            .init("allow_devices", "Extra allowed devices (for example c 10:232)", .list, required: false),
            .init("vms", "Only these VMs (empty: all)", .list, required: false),
        ]
        return f
    }

    static let vmmGuard = ResourceSpec(
        "bpf-guard", "VMM guard", group: "Security", icon: "lock.shield", endpoint: .get("/api/v1/bpf/guard"), layout: .keyValue,
        actions: [ResourceAction("Turn guard off", "PUT", "/api/v1/bpf/guard",
                                 staticBody: obj(["enabled": .bool(false), "mode": .string("audit"), "lease_secs": .null]))
            .with { $0.adminOnly = true; $0.global = true
                $0.confirm = "Disables the VMM guard on this host: no more audit records and no blocking of QEMU exec, W+X mappings or device opens." }],
        create: ResourceCreate("Audit mode…", "/api/v1/bpf/guard", controller: false, fields: guardFields(enforce: false)).with {
            $0.method = "PUT"; $0.adminOnly = true
            $0.fixed = ["enabled": .bool(true), "mode": .string("audit"), "lease_secs": .null]
            $0.warning = "Audit: turns the guard on and records what it would block. Nothing is denied. Replaces the whole guard configuration."
        },
        moreCreates: [ResourceCreate("Enforce under a lease…", "/api/v1/bpf/guard", controller: false, fields: guardFields(enforce: true)).with {
            $0.method = "PUT"; $0.adminOnly = true
            $0.fixed = ["enabled": .bool(true), "mode": .string("enforce")]
            $0.requiredKeys = ["lease_secs"]
            $0.typedPhrase = "ENFORCE GUARD"
            $0.warning = "ENFORCE: QEMU processes that exec another binary, map memory writable and executable, or open an unlisted device are denied until the lease ends, then the guard returns to audit by itself. Needs BPF-LSM active on the host. Run audit first and check the events."
        }],
        refreshSecs: 5)

    // MARK: XDP DDoS shield (`PUT /api/v1/bpf/shield`)

    private static func shieldFields() -> [ResourceCreate.Field] {
        [
            .init("iface", "Uplink interface (for example eth0)"),
            .init("protect_all", "Protect every address", .bool, initial: "no", required: false),
            .init("protected", "Protected addresses", .list, required: false),
            .init("syn_pps", "SYN per second per source", .int, initial: "1000", required: false, range: 0...10_000_000),
            .init("udp_pps", "UDP per second per source", .int, initial: "5000", required: false, range: 0...10_000_000),
            .init("icmp_pps", "ICMP per second per source", .int, initial: "100", required: false, range: 0...10_000_000),
            .init("other_pps", "Other per second (0 = unlimited)", .int, initial: "0", required: false, range: 0...10_000_000),
            .init("burst_secs", "Burst (seconds, 1 to 60)", .int, initial: "2", required: false, range: 1...60),
            .init("allow", "Allow list: sources that are never limited (your own address)", .list, required: false),
            .init("deny", "Deny list (CIDRs dropped outright)", .list, required: false),
        ]
    }

    static let bpfShield = ResourceSpec(
        "bpf-shield", "XDP DDoS shield", group: "Security", icon: "shield.checkered", endpoint: .get("/api/v1/bpf/shield"), layout: .keyValue,
        actions: [ResourceAction("Turn shield off", "PUT", "/api/v1/bpf/shield", staticBody: obj(["mode": .string("off")]),
                                 inputs: [.init("iface", "Uplink interface it is attached to")],
                                 confirm: "Detaches the shield from that interface: no more rate limiting or audit counters.", global: true)
            .with { $0.adminOnly = true }],
        create: ResourceCreate("Audit shield…", "/api/v1/bpf/shield", controller: false, fields: shieldFields()).with {
            $0.method = "PUT"; $0.adminOnly = true; $0.fixed = ["mode": .string("audit")]; $0.shape = .shield
            $0.requireAnyOf = ["protect_all", "protected"]
            $0.warning = "Audit: counts the packets that would be dropped and drops nothing. Check the counters and top sources before enforcing."
        },
        moreCreates: [ResourceCreate("Enforce shield (drops traffic)…", "/api/v1/bpf/shield", controller: false, fields: shieldFields()).with {
            $0.method = "PUT"; $0.adminOnly = true; $0.fixed = ["mode": .string("enforce")]; $0.shape = .shield
            $0.requiredKeys = ["allow"]; $0.requireAnyOf = ["protect_all", "protected"]
            $0.typedPhrase = "ENFORCE SHIELD"
            $0.warning = "ENFORCE: over-rate, denied and malformed packets are dropped at the uplink, but only while the enforcement lease is live (set it on Enforcement mode). The allow list must hold the address you manage this host from (this Mac, your jump host) or you can lose access. Audit first."
        }],
        refreshSecs: 5)

    // MARK: sched_ext VM scheduler (`PUT /api/v1/bpf/scx`)

    static let bpfScx = ResourceSpec(
        "bpf-scx", "VM scheduler (sched_ext)", group: "Security", icon: "cpu", endpoint: .get("/api/v1/bpf/scx"), layout: .keyValue,
        actions: [ResourceAction("Stop scheduler", "PUT", "/api/v1/bpf/scx", staticBody: obj(["enabled": .bool(false)]))
            .with { $0.adminOnly = true; $0.global = true
                $0.confirm = "Stops the helper and moves the listed VMs' vCPU threads back to the default (fair) scheduler class." }],
        create: ResourceCreate("Start under a lease…", "/api/v1/bpf/scx", controller: false, fields: [
            .init("vms", "VMs to schedule", .list),
            .init("lease_secs", "Lease (seconds, 1 to 3600)", .int, initial: "300", range: scxLeaseRange),
            .init("latency_target_us", "Latency target (microseconds)", .int, required: false, range: 1...10_000_000),
        ]).with {
            $0.method = "PUT"; $0.adminOnly = true; $0.fixed = ["enabled": .bool(true)]
            $0.requiredKeys = ["vms", "lease_secs"]
            $0.warning = "Moves only the listed VMs' vCPU threads (CPU n/KVM) to the machina sched_ext BPF scheduler. Everything else stays on the fair class. When the lease ends, the helper exits or the kernel ejects it, they go back to the fair class by themselves."
        },
        refreshSecs: 5)

    // MARK: VM edge (`PUT /api/v1/bpf/vm-edge`, replaces the whole state)

    static let bpfVmEdge = ResourceSpec(
        "bpf-vmedge", "VM edge", group: "Security", icon: "rectangle.connected.to.line.below", endpoint: .get("/api/v1/bpf/vm-edge"),
        rowsPath: ["taps"], idKey: "iface",
        columns: [.init("VM", "vm"), .init("Tap", "iface"), .init("Identity", "identity"), .init("Denied", "stats.denied"),
                  .init("Observed", "stats.observed"), .init("Rate dropped", "stats.rate_dropped"),
                  .init("Out pkts", "stats.out_pkts"), .init("In pkts", "stats.in_pkts")],
        actions: [ResourceAction("Clear VM edge state", "PUT", "/api/v1/bpf/vm-edge",
                                 staticBody: obj(["vms": .array([]), "policy": .array([])]),
                                 confirm: "Removes every VM from the edge on this host, including entries the controller synced: rate limits and isolation stop. The controller may push its state again on its next sync.", global: true)
            .with { $0.adminOnly = true }],
        create: ResourceCreate("Set one VM's edge (replaces state)…", "/api/v1/bpf/vm-edge", controller: false, fields: [
            .init("name", "VM name"),
            .init("group", "Group (policy identity)", required: false),
            .init("addresses", "Guest addresses", .list, required: false),
            .init("isolate_ingress", "Isolate ingress (default deny in)", .bool, initial: "no", required: false),
            .init("isolate_egress", "Isolate egress (default deny out)", .bool, initial: "no", required: false),
            .init("egress_mbps", "Egress Mbit/s (0 = unlimited)", .int, required: false, range: 0...1_000_000),
            .init("ingress_mbps", "Ingress Mbit/s (0 = unlimited)", .int, required: false, range: 0...1_000_000),
            .init("pps", "Packets/s each direction (0 = unlimited)", .int, required: false, range: 0...100_000_000),
        ]).with {
            $0.method = "PUT"; $0.adminOnly = true; $0.shape = .vmEdge
            $0.typedPhrase = "REPLACE VM EDGE"
            $0.warning = "This REPLACES the host's whole VM edge state with this single VM and no policy rules; VMs the controller synced are removed until its next sync. Rate limits apply immediately. Isolation drops traffic only while the enforcement lease is live, otherwise it is only counted as observed."
        },
        refreshSecs: 10)
}

extension ResourceCreate.BodyShape {
    func apply(_ flat: JSONValue) throws -> JSONValue {
        let o = flat.object ?? [:]
        switch self {
        case .fleet(let op): return try FleetEbpf.wrap(op: op, flat)
        case .shield:
            for k in ["allow", "deny", "protected"] {
                for item in o[k]?.array ?? [] where !EbpfForms.isAddressOrCIDR(item.text) {
                    throw ResourceCreate.Invalid(message: "\(k): \(item.text) is not an IP address or CIDR.")
                }
            }
            var out = o
            if out["protect_all"] == nil { out["protect_all"] = .bool(false) }
            return .object(out)
        case .vmEdge:
            var vm = o
            for item in o["addresses"]?.array ?? [] where !EbpfForms.isAddressOrCIDR(item.text) {
                throw ResourceCreate.Invalid(message: "addresses: \(item.text) is not an IP address.")
            }
            vm["isolate_ingress"] = vm["isolate_ingress"] ?? .bool(false)
            vm["isolate_egress"] = vm["isolate_egress"] ?? .bool(false)
            return .object(["vms": .array([.object(vm)]), "policy": .array([])])
        }
    }
}

public enum EbpfForms {
    /// IPv4/IPv6 address with an optional `/prefix` within range.
    public static func isAddressOrCIDR(_ s: String) -> Bool {
        let parts = s.split(separator: "/", omittingEmptySubsequences: false)
        guard (1...2).contains(parts.count) else { return false }
        let addr = String(parts[0])
        var v4 = in_addr(); var v6 = in6_addr()
        let is4 = inet_pton(AF_INET, addr, &v4) == 1
        let is6 = !is4 && inet_pton(AF_INET6, addr, &v6) == 1
        guard is4 || is6 else { return false }
        if parts.count == 2 {
            guard let n = Int(parts[1]), n >= 0, n <= (is4 ? 32 : 128) else { return false }
        }
        return true
    }

    /// The four pages with their create forms and actions, for tests and audits.
    public static var writeSpecs: [ResourceSpec] {
        ["bpf-guard", "bpf-shield", "bpf-scx", "bpf-vmedge"].compactMap { FeatureRegistry.spec($0) }
    }
}
