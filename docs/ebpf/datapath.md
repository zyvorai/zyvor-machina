# Datapath: load balancing, shield, node isolation, health, TLS

Back to [native eBPF overview](README.md).

## Uplink XDP dispatcher

Everything that runs at XDP on the host uplink shares one program,
`mn_xdp_uplink`, so features compose instead of fighting over the attach point.
Order of evaluation:

1. **Node isolation** (`XDP_F_NODEISO`) — emergency drop-all, checked first.
2. **DDoS shield** — per-source token buckets.
3. **QUIC-LB** (`XDP_F_QUICLB`, tail call) — see [fastpath.md](fastpath.md).
4. **NodePort** to local backends when `MACHINA_CNI_XDP=1` — see [cni.md](cni.md).

Only one uplink per host is supported.

## Service load balancing (Maglev)

Kubernetes Services synced by `machina-cni` are load-balanced in bpfd:

- ClusterIP / externalIP / LoadBalancer go through **cgroup socket-LB** (the
  connect is rewritten before a packet exists); NodePort is handled by tc on
  the uplink, or at XDP with `MACHINA_CNI_XDP=1`.
- Multi-backend services get a **Maglev** table (M = 1021, built from sorted
  backends) so backend changes move as few flows as possible.
- `sessionAffinity: ClientIP` honours `timeoutSeconds`.
- Remote backends: `MACHINA_CNI_LB_MODE=snat` (default) or `dsr` (IPIP with the
  NodePort carried in the outer IP ID; IPv4 only).

Inspect: `GET /api/v1/bpf/cni/services` lists each synced service with its
Maglev table and live affinity entries. UI: Native eBPF → **Service LB**.

The same Maglev implementation backs [QUIC-LB](fastpath.md#quic-lb). Fleet
Cloud load balancers are a separate, iptables-based engine
(`controller/src/engine/load_balancer.rs`).

## DDoS shield

`PUT /api/v1/bpf/shield` (`server/shield.rs`):

```json
{
  "iface": "eno1",
  "mode": "audit",
  "protect_all": true,
  "syn_pps": 1000, "udp_pps": 5000, "icmp_pps": 100, "other_pps": 0,
  "burst_secs": 2,
  "allow": ["10.0.0.0/8"], "deny": []
}
```

- Per-source, per-class token buckets in an LRU (`SHIELD_SOURCES`, 64k
  entries). `other_pps: 0` means unlimited. `protected` limits it to listed
  destination addresses instead of `protect_all`.
- `mode: off | audit | enforce`. Over-rate, denied and malformed packets drop
  only in `enforce` **with the enforcement lease live**; otherwise they count
  as `audited`.
- `GET` returns counters and the top over-rate sources. UI tab **Shield**.

## Node isolation

An emergency "cut this host off" switch on the uplink
(`server/nodeiso.rs`). `PUT /api/v1/bpf/node-iso`:

```json
{
  "enabled": true, "iface": "eno1", "lease_secs": 300, "dry_run": true,
  "allow_tcp": [22, 6443, 5092, 5093, 50051, 10250],
  "allow_udp": [], "exempt": ["192.0.2.10/32"], "allow_icmp": true
}
```

- Ingress is the head of `mn_xdp_uplink`; egress is `mn_nodeiso`, attached
  **first** on TCX.
- Non-IP, IPv6 neighbour discovery and DHCP always pass. Allowlisted ports
  match either side, so existing SSH and API sessions survive. Exempt CIDRs
  bypass entirely.
- **Refuses to enable** unless TCP 22 is allowlisted or an exempt CIDR is set.
- Its own lease (`lease_secs`, mandatory, 10–900 s): the datapath stops
  dropping at the deadline by itself and the maintenance tick detaches
  (`lease_expired: true`). Not persisted.
- UI tab **Node Isolation** defaults to dry run and blocks arming without SSH or
  an exempt CIDR. Never test on a real uplink.

## TCP and ICMP health (observe only)

- `mn_sockops` on the root cgroup (telemetry `tcp`, on by default;
  `MACHINA_BPF_SOCKOPS_CGROUP` overrides) times every active connect per remote
  `addr:port` (count, failures, average, max, 8-bucket histogram) and snapshots
  srtt, cwnd, ssthresh, MSS, retransmits and delivery rate per peer.
- `GET /api/v1/bpf/health` returns `connect` and `pressure`.
- The tc programs count ICMP unreachable / time-exceeded / parameter-problem /
  packet-too-big per interface and direction: `GET /api/v1/bpf/icmp-errors`.
- UI tab **TCP Health**.

## TLS fingerprints and OpenSSL L7 (off by default)

`PUT /api/v1/bpf/tls`:

```json
{
  "fingerprints": true, "fingerprint_rate": 50,
  "ssl_uprobes": false, "ssl_comms": ["curl"], "ssl_all_processes": false,
  "ssl_rate": 200
}
```

- `fingerprints` attaches `mn_tlsfp` (cgroup_skb egress, root cgroup;
  `MACHINA_BPF_TLSFP_CGROUP` overrides), samples up to 2 KB of each
  ClientHello, and computes **JA3** (+ md5) and **JA4** with SNI, ALPN and the
  owning workload. `GET /api/v1/bpf/tls/fingerprints`.
- `ssl_uprobes` probes `SSL_write(_ex)` / `SSL_read(_ex)` in every libssl found
  through `/proc/*/maps` (including container copies), rescanned every 30 s.
  Only processes in `ssl_comms` are captured, and only the HTTP method, host,
  path and status leave bpfd (`GET /api/v1/bpf/tls/ssl`, admin); the
  plaintext head is dropped after parsing.
- Fleet view: `GET /api/v1/zeus-security/tls/fingerprints` (records + `top_ja4`).
- UI tab **TLS / JA4**.

## Workload attribution

Flow, event, DNS, L7, process, TLS and SSL records carry
`workload: {kind: vm|pod|container|service, ns?, name}`: taps map to VMs, CNI
host veths to pods, `machine-qemu*` scopes to VMs, kubepods cgroups to pods
(joined to `ns/name` from kubelet's pod log directories), runtime scopes to
containers and `.service` units to services.
