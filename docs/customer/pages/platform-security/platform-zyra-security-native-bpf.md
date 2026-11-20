# Native eBPF

## Purpose

Operate Machina's built-in eBPF datapath on a host. One root service, `machina-bpfd`, loads Machina's own kernel programs for flow visibility, policy enforcement, load balancing, DDoS protection, VM isolation and scheduling. This page is the console for it; there is no separate Cilium, Tetragon, Netra or PacketWolf agent to install.

## When to use it

- See which VMs talk to what (flows, DNS, L7, per-VM byte accounting)
- Block or allow traffic with policies, first in **observe**, then **enforce** under a lease
- Load-balance services on the uplink, absorb floods (Shield), or isolate a node during an incident
- Check what the running kernel supports before turning on a feature

## How to get there

- Route: `/platform/zyra/security/native-bpf`
- Nav: **Security Center → Native eBPF** (or spotlight / Finder search for "eBPF")
- Per-VM container rules live in a different place: VM detail → **Guest policy** (see [Guest policy](../core/vm-guest-policy.md))

## Before you start

- `machina-bpfd` must be running on the host (`systemctl status machina-bpfd`).
  If it is not, the page shows "bpfd unavailable" and every tab stays empty.
- **Overview → programs compiled = no** means the build had no eBPF toolchain;
  the admin runs `make bpf-deps` and rebuilds.
- Viewing needs any logged-in role. Every change (mode, policies, attach, leases) is **Admin** only.

## Tabs

The page has 22 tabs. The first eight are the core console; the rest are one tab per datapath feature.

| Tab | What it shows / does |
|---|---|
| **Overview** | Version, mode (observe / enforce + lease left), kernel feature pills (`btf`, `tcx`, `lsm_bpf`, `sched_ext`, `xsk`…), policy count, attached interfaces, flow / drop / anomaly counters. Switch mode and manage policies here. |
| **Flows** | Live connection table per VM (5-tuple, bytes, verdict). Filter by VM. |
| **L7** | Parsed HTTP / DNS / TLS-SNI requests by protocol. |
| **Accounting** | Per-VM byte and packet counters; reset one VM or all. |
| **Live** | Streaming events (net, dns, l7, proc, anomaly topics) over SSE. |
| **DNS & processes** | DNS answers seen and which host processes opened sockets. |
| **Captures** | Start a ring-buffer packet capture on one interface and download it as pcapng for Wireshark. |
| **QoS & telemetry** | Per-VM rate limits and the datapath telemetry counters. |
| **Service LB** | Uplink service load balancing (VIP → backends, weighted, with health). |
| **VM Edge** | Per-VM tap programs: anti-spoof, isolation, QEMU sandbox. |
| **Shield** | XDP DDoS shield on the uplink: per-source SYN / UDP / ICMP rate limits with allow and deny lists; shows the top over-rate sources. |
| **TCP Health** | Retransmits, RTT and ICMP errors per peer. |
| **TLS / JA4** | TLS client fingerprints (JA4) and SNI seen per VM. |
| **Node Isolation** | Incident kill-switch for the host: drop everything except an allowlist, with its own 10–900 s lease. Refuses to arm unless SSH or an exempt CIDR is allowlisted. |
| **Net changes** | Audit of netlink changes (links, addresses, routes) on the host. |
| **L7 sampling** | Sampled L7 payload metadata for low-overhead visibility. |
| **VM runtime** | Per-VM runtime intelligence (vCPU exits, block and network activity from QEMU). |
| **VMM guard** | BPF-LSM guard around QEMU processes (audit, or enforce under a lease; needs `lsm_bpf`). |
| **Direct redirect** | Opt-in per VM: frames for the VM on an outer interface go straight into its tap (and back with reverse), bypassing the bridge. Idle unless the enforcement lease is live. |
| **QUIC LB** | Connection-ID aware QUIC load balancing on the uplink. |
| **AF_XDP** | Zero-copy AF_XDP sockets for a chosen interface (never the default-route NIC). |
| **Scheduler** | `machina-scx` sched_ext VM scheduler: status, start / stop under a lease (needs `sched_ext`). |

## Safe workflow for enforcement

1. Add the policy while the host is in **observe**. Misses are counted, nothing is dropped.
2. Watch **Flows** / **Live** for the hits you expect. Fix the rule until only unwanted traffic would be blocked.
3. Switch to **enforce** with a lease (default 900 s). The lease is checked in the datapath, so drops stop on time even if bpfd stalls.
4. Extend the lease only once you are sure. A bpfd restart always comes back in observe: mode and leases are never persisted.

Guard rails you will see: auto-attach only touches VM taps (`vnet*` / `tap*`),
default-deny is never attached to an uplink, and direct redirect refuses a
physical NIC unless forced. Test new enforcement on a lab host first.

## Fleet view

On the controller, **Security Center** aggregates every host's dataplane
(`/api/v1/zeus-security/native-dataplane`) and pushes per-host changes through
the host's agent. Use this page for one host, Security Center for the fleet.

## If something is wrong

- **bpfd unavailable:** start the unit, then check `journalctl -u machina-bpfd`.
- **A tab says the kernel lacks a feature:** compare the Overview feature pills with the kernel requirements in the admin docs.
- **Enforcement went back to observe:** the lease expired or bpfd restarted. This is by design.

## Related pages

- [Security Center](platform-zeus-security.md)
- [Guest policy](../core/vm-guest-policy.md)
- [Network Canvas](../platform/platform-network-canvas.md)
- [Getting Started](../../getting-started.md)
- [Page index](../../PAGE_INDEX.md)
