import Foundation

/// Phase 9 pages: autopilot, chaos, preemptible instances, topology, the AI operator and incidents, bare metal
/// provisioning with BMC power, and lease-aware eBPF enforcement. Every path and body below was checked against
/// `web/src/api/*.ts` and the controller / daemon route tables (`controller/src/api/mod.rs`,
/// `daemon/src/routes/bpf.rs`); the notes say which handler and role each one maps to.
extension FeatureRegistry {
    private static func obj(_ pairs: [String: JSONValue]) -> JSONValue { .object(pairs) }

    /// Longest the app waits for the slow controller reports (incident correlation, performance analysis).
    static let slowReport: TimeInterval = 90

    static let opsSpecs: [ResourceSpec] = [
        // MARK: Autopilot (controller `api/ai.rs`: operator role)
        ResourceSpec("ap-proposals", "Autopilot proposals", group: "Autopilot", icon: "wand.and.stars",
                     endpoint: Endpoint("GET", "/api/v1/ai/autopilot/propose", controller: true, timeout: 60), rowsPath: ["actions"], idKey: "id",
                     columns: [.init("Action", "label"), .init("Risk", "risk"), .init("Type", "action_type"), .init("Review", "review")],
                     actions: [
                        ResourceAction("Execute", "/api/v1/ai/autopilot/execute", destructive: true, controller: true,
                                       bodyFields: ["action_type": "action_type", "object_ref": "object_ref"])
                            .with { $0.confirm = "Runs this proposed action on the controller now. Read the Review column first." },
                        ResourceAction("Run safe actions", "/api/v1/ai/autopilot/run", destructive: true, controller: true,
                                       staticBody: obj(["max_actions": .number(3)]))
                            .with { $0.rowless = true; $0.confirm = "Lets the autopilot execute up to 3 of the actions it classes as safe, without asking again." },
                     ], refreshSecs: 60),
        ResourceSpec("ap-performance", "Performance autopilot", group: "Autopilot", icon: "speedometer",
                     endpoint: .controllerGet("/api/v1/vms"),
                     columns: [.init("VM", "_parent"), .init("Action", "title"), .init("Priority", "priority"), .init("Score", "score"),
                               .init("Benefit", "expected_benefit"), .init("Risk", "risk"), .init("Approval", "requires_approval"), .init("Reversible", "reversible")],
                     fanOut: .init(parent: .controllerGet("/api/v1/vms"), childPath: "/api/v1/ai/performance-autopilot",
                                   childQuery: [.init(name: "vm_id", value: "{id}")], childRowsPath: ["actions"],
                                   filterKey: "observed_state", filterValue: "running", limit: 12, childTimeout: slowReport),
                     refreshSecs: 0),
        ResourceSpec("ap-history", "Autopilot history", group: "Autopilot", icon: "clock.arrow.circlepath",
                     endpoint: Endpoint("GET", "/api/v1/ai/autopilot/history", query: [.init(name: "limit", value: "50")], controller: true), idKey: "id",
                     columns: [.init("Time", "created_at", .date), .init("Actor", "actor"), .init("Action", "action"), .init("Detail", "detail")]),

        // MARK: Resilience: chaos / game days (controller `api/chaos.rs`; run and abort need operator)
        ResourceSpec("chaos-experiments", "Chaos experiments", group: "Resilience", icon: "bolt.trianglebadge.exclamationmark",
                     endpoint: .controllerGet("/api/v1/chaos/experiments"), idKey: "id",
                     columns: [.init("Name", "name"), .init("Description", "description"), .init("Targets", "spec.targets"), .init("Steps", "spec.steps"),
                               .init("Last status", "last_status"), .init("Last run", "last_run_at", .date), .init("Created by", "created_by")],
                     actions: [
                        ResourceAction("Run", "/api/v1/chaos/experiments/{id}/run", destructive: true, controller: true, bodyFields: ["confirm": "name"])
                            .with { $0.typeToConfirm = "name"; $0.confirm = "Injects REAL faults (latency, loss, partitions, disk throttling or a VM kill) into the targets named by this experiment. It aborts by itself if its probes fall below the experiment's threshold." },
                        ResourceAction("Delete", "DELETE", "/api/v1/chaos/experiments/{id}", destructive: true, controller: true),
                     ]),
        ResourceSpec("chaos-runs", "Chaos runs", group: "Resilience", icon: "play.circle", endpoint: .controllerGet("/api/v1/chaos/runs"), idKey: "id",
                     columns: [.init("Started", "started_at", .date), .init("Status", "status"), .init("By", "started_by"), .init("Verdict", "report.verdict"),
                               .init("Duration s", "report.duration_s"), .init("Findings", "report.findings"), .init("Abort reason", "abort_reason")],
                     actions: [ResourceAction("Abort", "/api/v1/chaos/runs/{id}/abort", destructive: true, controller: true, staticBody: obj([:]))
                        .with { $0.confirm = "Stops the run and removes its faults." }],
                     refreshSecs: 10),
        ResourceSpec("chaos-faults", "Active faults", group: "Resilience", icon: "exclamationmark.triangle",
                     endpoint: .controllerGet("/api/v1/chaos/faults"), rowsPath: ["items"], idKey: "id",
                     columns: [.init("VM", "vm"), .init("Host", "host"), .init("Delay ms", "delay_ms"), .init("Jitter ms", "jitter_ms"), .init("Loss %", "loss_pct"),
                               .init("Partition", "partition"), .init("Remaining s", "remaining_secs"), .init("Error", "error")],
                     refreshSecs: 5),

        // MARK: Resilience: preemptible / spot (controller `api/preempt.rs`: settings and per-VM flag need operator)
        ResourceSpec("preempt-settings", "Preemption settings", group: "Resilience", icon: "arrow.down.circle.dotted",
                     endpoint: .controllerGet("/api/v1/preemption"), rowsPath: ["settings"], layout: .keyValue,
                     create: ResourceCreate("Change settings…", "/api/v1/preemption/settings",
                                            fields: [.init("enabled", "Preemption enabled", .bool, initial: "yes"),
                                                     .init("reserve_pct", "Free memory to reserve (%)", .int, initial: "10", range: 0...90)])
                        .with { $0.method = "PUT"; $0.warning = "With preemption on, the controller can stop preemptible VMs to free memory for others." }),
        ResourceSpec("preempt-vms", "Preemptible VMs", group: "Resilience", icon: "tag", endpoint: .controllerGet("/api/v1/preemption"), rowsPath: ["vms"], idKey: "id",
                     columns: [.init("Name", "name"), .init("Host", "host"), .init("Project", "project"), .init("Memory MiB", "memory_mib"), .init("Priority", "priority"),
                               .init("State", "observed_state"), .init("Desired", "desired_state"), .init("Preempted at", "preempted_at", .date)],
                     actions: [ResourceAction("Make non-preemptible", "PUT", "/api/v1/vms/{id}/preemptible", destructive: true, controller: true,
                                              bodyFields: ["priority": "priority"], staticBody: obj(["preemptible": .bool(false)]))
                        .with { $0.confirm = "The VM is no longer a candidate for preemption. A preempted VM is resumed." }]),
        ResourceSpec("preempt-hosts", "Preemption headroom", group: "Resilience", icon: "memorychip", endpoint: .controllerGet("/api/v1/preemption"), rowsPath: ["hosts"], idKey: "id",
                     columns: [.init("Host", "name"), .init("Total MiB", "total_mib"), .init("Used MiB", "used_mib"), .init("Free MiB", "free_mib"),
                               .init("Reserve MiB", "reserve_mib"), .init("Preemptible running MiB", "preemptible_running_mib"), .init("Fresh data", "fresh")]),
        ResourceSpec("preempt-events", "Preemption events", group: "Resilience", icon: "list.bullet.clipboard", endpoint: .controllerGet("/api/v1/preemption"), rowsPath: ["events"],
                     columns: [.init("Time", "at", .date), .init("VM", "vm"), .init("Kind", "kind"), .init("Reason", "reason")]),

        // MARK: Resilience: topology (controller `api/topology.rs`)
        ResourceSpec("topology-nodes", "Topology nodes", group: "Resilience", icon: "point.3.connected.trianglepath.dotted",
                     endpoint: .controllerGet("/api/v1/topology"), rowsPath: ["nodes"], idKey: "id",
                     columns: [.init("Kind", "kind"), .init("Name", "name"), .init("State", "state"), .init("ID", "id")]),
        ResourceSpec("topology-edges", "Topology links", group: "Resilience", icon: "arrow.left.and.right", endpoint: .controllerGet("/api/v1/topology"), rowsPath: ["edges"],
                     columns: [.init("From", "from"), .init("To", "to"), .init("Label", "label")]),
        ResourceSpec("topology-warnings", "Topology warnings", group: "Resilience", icon: "exclamationmark.bubble", endpoint: .controllerGet("/api/v1/topology"), rowsPath: ["warnings"],
                     columns: [.init("Severity", "severity"), .init("Message", "message"), .init("Suggested fix", "fix_action")]),

        // MARK: AI operator and incidents (controller `api/ai.rs`, operator role)
        ResourceSpec("ai-pending", "AI actions awaiting approval", group: "AI operator", icon: "checkmark.seal",
                     endpoint: .controllerGet("/api/v1/ai/actions/hub"), rowsPath: ["zyra_actions"], idKey: "id",
                     columns: [.init("Action", "label"), .init("Type", "action_type"), .init("Risk", "risk"), .init("Status", "status"), .init("Source", "source"), .init("Review", "review")],
                     actions: [ResourceAction("Approve and run", "/api/v1/ai/actions/{id}/execute", destructive: true, controller: true)
                                .with { $0.confirm = "Approves this AI-proposed action and runs it now. Read the Review column first." },
                               ResourceAction("Reject", "/api/v1/ai/actions/{id}/reject", destructive: true, controller: true)]),
        ResourceSpec("ai-agents", "AI operator", group: "AI operator", icon: "sparkles", endpoint: .controllerGet("/api/v1/ai/agents"), idKey: "id",
                     columns: [.init("Agent", "name"), .init("ID", "id"), .init("Task class", "task_class"), .init("Description", "description")],
                     create: ResourceCreate("Ask the AI operator…", "/api/v1/ai/zyra/chat",
                                            fields: [.init("message", "Question"), .init("agent", "Agent (auto picks one)", initial: "auto", required: false)])
                        .with { $0.timeout = 120; $0.resultKey = "reply" }),
        ResourceSpec("ai-incidents", "Active incidents", group: "AI operator", icon: "cross.case",
                     endpoint: Endpoint("GET", "/api/v1/ai/incidents/active", controller: true, timeout: slowReport), idKey: "id",
                     columns: [.init("Opened", "created_at", .date), .init("Severity", "severity"), .init("Status", "status"), .init("Title", "title"),
                               .init("Affected", "affected_resources"), .init("Root cause", "root_cause"), .init("Summary", "summary")],
                     actions: [ResourceAction("Acknowledge", "/api/v1/ai/incidents/{id}/ack", destructive: true, controller: true)
                        .with { $0.confirm = "Marks the incident acknowledged for everyone." }],
                     refreshSecs: 60),
        ResourceSpec("ai-incident-timeline", "Incident analysis (24 h)", group: "AI operator", icon: "timeline.selection",
                     endpoint: Endpoint("GET", "/api/v1/ai/incidents/analyze", query: [.init(name: "hours", value: "24")], controller: true, timeout: slowReport),
                     rowsPath: ["timeline"],
                     columns: [.init("Time", "at", .date), .init("Severity", "severity"), .init("Source", "source"), .init("Kind", "kind"), .init("Message", "message")],
                     refreshSecs: 0),
    ]

    // MARK: Bare metal (controller `api/baremetal.rs`: power, commission, provision need admin; release needs operator)

    private static let powerPath = "/api/v1/baremetal/servers/{id}/power"

    static let bareMetalServers = ResourceSpec(
        "bm-servers", "Servers", group: "Bare metal", icon: "server.rack", endpoint: .controllerGet("/api/v1/baremetal/servers"), idKey: "id",
        columns: [.init("Hostname", "hostname"), .init("BMC", "bmc_address"), .init("Type", "bmc_type"), .init("Lifecycle", "lifecycle"), .init("State", "state"),
                  .init("Zone", "zone"), .init("MAC", "mac_address"), .init("CPU", "cpu_cores"), .init("Memory MiB", "memory_mib"),
                  .init("BMC login", "bmc_credentials_configured"), .init("Last error", "last_error")],
        actions: [
            ResourceAction("Dry-run power cycle", powerPath, controller: true, staticBody: obj(["action": .string("cycle"), "dry_run": .bool(true)]))
                .with { $0.adminOnly = true; $0.confirm = "Asks the controller what a power cycle would do. Nothing is sent to the BMC." },
            ResourceAction("Power on", powerPath, destructive: true, controller: true, staticBody: obj(["action": .string("on")]))
                .with { $0.adminOnly = true; $0.confirm = "Sends a power-on command to this machine's BMC." },
            ResourceAction("Power off", powerPath, destructive: true, controller: true, staticBody: obj(["action": .string("off")]))
                .with { $0.adminOnly = true; $0.typeToConfirm = "hostname"; $0.confirm = "Cuts power through the BMC. The OS gets no chance to shut down." },
            ResourceAction("Power cycle", powerPath, destructive: true, controller: true, staticBody: obj(["action": .string("cycle")]))
                .with { $0.adminOnly = true; $0.typeToConfirm = "hostname"; $0.confirm = "Powers the machine off and on through the BMC. Anything running on it is interrupted." },
            ResourceAction("Hard reset", powerPath, destructive: true, controller: true, staticBody: obj(["action": .string("reset")]))
                .with { $0.adminOnly = true; $0.typeToConfirm = "hostname"; $0.confirm = "Resets the machine through the BMC without a clean shutdown." },
            ResourceAction("Commission", "/api/v1/baremetal/servers/{id}/commission", destructive: true, controller: true)
                .with { $0.adminOnly = true; $0.confirm = "Starts commissioning: the machine must PXE-boot the inventory image and report back with a one-time token." },
            ResourceAction("Release", "/api/v1/baremetal/servers/{id}/release", destructive: true, controller: true)
                .with { $0.confirm = "Returns an allocated or deployed server to the ready pool." },
        ],
        create: ResourceCreate("Register server…", "/api/v1/baremetal/servers",
                               fields: [.init("hostname", "Hostname"), .init("bmc_address", "BMC address (host or URL)"),
                                        .init("mac_address", "PXE NIC MAC", required: false), .init("zone", "Zone", required: false)])
            .with { $0.adminOnly = true; $0.warning = "Adds the record only. Set the BMC login in the web UI; the app never handles BMC passwords." })

    static let bareMetalProfiles = ResourceSpec(
        "bm-profiles", "Profiles", group: "Bare metal", icon: "slider.horizontal.3", endpoint: .controllerGet("/api/v1/baremetal/profiles"), idKey: "name",
        columns: [.init("Name", "name"), .init("Image", "image"), .init("Zone", "zone"), .init("Timezone", "timezone"), .init("Min CPU", "min_cpu_cores"),
                  .init("Min memory MiB", "min_memory_mib"), .init("Auto deploy", "auto_deploy"), .init("Desired count", "desired_count")],
        actions: [
            ResourceAction("Provision 1 machine", "/api/v1/baremetal/provision", destructive: true, controller: true,
                           bodyFields: ["profile": "name"], staticBody: obj(["count": .number(1)]))
                .with { $0.adminOnly = true; $0.typeToConfirm = "name"
                    $0.confirm = "Allocates one ready server for this profile and installs its image, WIPING the machine's disks. The deploy starts immediately." },
            ResourceAction("Delete profile", "DELETE", "/api/v1/baremetal/profiles/{name}", destructive: true, controller: true).with { $0.adminOnly = true },
        ])

    // MARK: eBPF enforcement (daemon `routes/bpf.rs`: every write needs admin; bpfd enforces leases)

    private static let enforceWarning = "Enforcement drops traffic that policies, the VM edge, the shield or the sandbox deny, but only until the lease runs out; then the datapath fails open by itself. The app always sends an explicit lease (60 to 900 s)."

    static let enforcementMode = ResourceSpec(
        "bpf-mode", "Enforcement mode", group: "Security", icon: "switch.2", endpoint: .get("/api/v1/bpf/status"), rowsPath: ["mode"], layout: .keyValue,
        actions: [ResourceAction("End enforcement (observe)", "PUT", "/api/v1/bpf/mode", staticBody: obj(["mode": .string("observe")]))
            .with { $0.adminOnly = true; $0.rowless = true; $0.confirm = "Returns the datapath to observe: it records verdicts but drops nothing." }],
        create: ResourceCreate("Enforce with a lease…", "/api/v1/bpf/mode", controller: false,
                               fields: [.init("lease_secs", "Lease (seconds, 60 to 900)", .int, initial: "300", range: 60...900)])
            .with { $0.method = "PUT"; $0.fixed = ["mode": .string("enforce")]; $0.adminOnly = true; $0.warning = enforceWarning },
        refreshSecs: 5)

    static let enforcementPolicies = ResourceSpec(
        "bpf-policies", "Enforcement policies", group: "Security", icon: "checklist.checked", endpoint: .get("/api/v1/bpf/policies"), idKey: "id",
        columns: [.init("Name", "name"), .init("ID", "id"), .init("Kind", "kind"), .init("Match", "match"), .init("Enabled", "enabled"),
                  .init("Scope", "scope.kind"), .init("Description", "description")],
        actions: [ResourceAction("Remove", "DELETE", "/api/v1/bpf/policies/{id}", destructive: true).with { $0.adminOnly = true }],
        create: ResourceCreate("Add policy…", "/api/v1/bpf/policies", controller: false,
                               fields: [.init("id", "ID (short slug)"), .init("name", "Name", required: false),
                                        .init("kind", "Kind", initial: "deny_ip",
                                              options: ["deny_ip", "deny_port", "allow_port", "deny_dns", "deny_process", "deny_file", "deny_cap", "rate_limit"]),
                                        .init("match", "Match (address, port, name or rate such as 100/s)"), .init("description", "Description", required: false)])
            .with { $0.fixed = ["enabled": .bool(true)]; $0.adminOnly = true
                $0.warning = "A policy only drops traffic while enforcement is on with a live lease. default-deny (tc_allow) is not offered here." })

    static let nodeIsolation = ResourceSpec(
        "bpf-nodeiso", "Node isolation", group: "Security", icon: "network.slash", endpoint: .get("/api/v1/bpf/node-iso"), layout: .keyValue,
        actions: [ResourceAction("Disable isolation", "PUT", "/api/v1/bpf/node-iso", staticBody: obj(["enabled": .bool(false)]))
            .with { $0.adminOnly = true; $0.rowless = true; $0.confirm = "Stops isolating this node." }],
        create: nodeIsoForm(arm: false),
        moreCreates: [nodeIsoForm(arm: true)],
        refreshSecs: 5)

    /// Dry run first (counts would-be drops); arming is a separate, typed-phrase form.
    static func nodeIsoForm(arm: Bool) -> ResourceCreate {
        let fields: [ResourceCreate.Field] = [
            .init("iface", "Uplink interface (for example eth0)"),
            .init("lease_secs", "Lease (seconds, 10 to 900)", .int, initial: arm ? "30" : "60", range: 10...900),
            .init("allow_tcp", "Allowed TCP ports", .intList, initial: "22,6443,5092,5093,50051,10250", required: false),
            .init("exempt", "Exempt CIDRs (your own address)", .list, required: false),
        ]
        return ResourceCreate(arm ? "Arm isolation (drops traffic)…" : "Dry run isolation…", "/api/v1/bpf/node-iso", controller: false, fields: fields).with {
            $0.method = "PUT"; $0.adminOnly = true
            $0.fixed = ["enabled": .bool(true), "dry_run": .bool(!arm)]
            if arm {
                $0.typedPhrase = "ISOLATE NODE"; $0.guardLockout = true
                $0.warning = "DROPS ALL TRAFFIC on the uplink except the allowlist and exempt CIDRs, until the lease ends. Include port 5092 (this app) and 22, or an exempt CIDR for this Mac, or you will lose access. Use a dry run first."
            } else {
                $0.warning = "Dry run: counts the packets it would drop and drops nothing. Check 'would_drop' before arming."
            }
        }
    }
}
