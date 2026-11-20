# Load balancing: Rivora spike

**Question.** Can Rivora (our eBPF/XDP L4 balancer, `../rivora`) be the datapath behind Fleet Cloud load balancers, replacing
the host-iptables round-robin in `engine/load_balancer.rs`?

**What was run** (lab host, Linux 7.0.0-34, BTF + TCX present, Cilium on the uplink): Rivora built from source (Go 1.27.1
cross-compiled for linux/amd64, BPF objects compiled on the host with clang), then its own self-tests as root. They build an
isolated topology (fresh network namespaces, veth pairs and a bridge created for the test, never a host interface) and drive
real TCP traffic through the VIP. Nothing outside that topology was touched; Cilium's TCX programs on the uplink were left alone.

| Self-test | Result |
|---|---|
| `rivora-doctor --require-tcx` | 6 pass (root, kernel/TCX, bpffs, BTF, clang, bpftool) |
| `selftest.sh`: DSR mode | pass: Maglev spread 10/10 of 20; failover removed a dead backend, all traffic reached the survivor |
| `selftest.sh`: full-NAT mode | pass: spread 11/9 of 20; same failover result |
| `selftest-weighted.sh` (9:1) | pass: 179/200 to the weight-9 backend |
| `selftest-httpcheck.sh` | 18/18 pass: TCP and HTTP probes, a failing health endpoint with an open service port marks the backend down, recovery, probe-path change by SIGHUP reload, bad config refused at start-up |

**Conclusion.** Rivora runs on this kernel and does what the plan needs from the datapath: Maglev spread, weights, NAT mode,
active TCP/HTTP health checks that drive failover and recovery, and config reload without a restart. The spike did **not** put
Rivora on a libvirt VPC bridge with real guests, so these remain open before committing to it:

1. **Attach point on a libvirt bridge.** The self-tests use a bridge built for the test; a libvirt `virbr`/`mc-*` bridge with
   TAPs needs checking (XDP on bridge devices is generic/skb mode; TCX on the bridge or its ports is the alternative).
2. **Coexistence with `machina-bpfd`**, which owns the uplink's XDP slot (`mn_xdp_uplink`: shield, NodePort, node isolation).
   Rivora must attach to the VPC bridge or taps, or share the dispatcher.
3. **One config per host**, so a balancer's targets must be on its host (as today) until a VIP-per-host or BGP/ECMP scheme exists.
4. **Packaging and signing** of a Go binary next to the Rust ones (release pipeline: build, SBOM, cosign).

**Integration plan if we proceed.** Keep the model in Machina (balancer, listeners, members, the health check now in
`engine/lb_health.rs`); render Rivora's YAML (`vips[]`, `backends[]`, `healthCheck`) in the controller, push it with a new agent
action (`lb.rivora.apply`) that writes the file and signals a reload, and read per-backend health back through Rivora's API into
the member health shown today. Drain and weight changes go through its API without a reload. The iptables engine stays as the
fallback backend so existing balancers do not change until moved.

Until then the iptables balancer gets the same health checks, in the agent, so members that fail leave rotation today.
