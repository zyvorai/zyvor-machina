# Observability: flows, L7, audit and VM runtime

Back to [native eBPF overview](README.md). Everything on this page is observe
only; none of it can drop traffic.

## Flow and event telemetry

| Route | Returns |
|---|---|
| `GET /api/v1/bpf/flows` | Per-flow counters with workload attribution |
| `GET /api/v1/bpf/events` | Network and policy events |
| `GET /api/v1/bpf/dns` | DNS queries and answers |
| `GET /api/v1/bpf/l7` | TLS SNI/ALPN, HTTP request line, SSH banner from each TCP flow's first client payload |
| `GET /api/v1/bpf/processes` | Process exec/exit records |
| `GET /api/v1/bpf/anomalies` | Detected anomalies |
| `GET /api/v1/bpf/accounting` | Per-VM tx/rx/drops, folded every 60 s and persisted (`POST …/accounting/reset`) |
| `GET /api/v1/bpf/stream` | Server-sent events of all of the above |
| `GET`/`PUT /api/v1/bpf/telemetry` | Which collectors run |

Captures: `POST /api/v1/bpf/captures` starts a filtered capture,
`GET /api/v1/bpf/captures/{id}` downloads pcapng.

The controller ingests these into the SOC (`machina.bpf` dataset), the network
canvas and AI root-cause. TCP connect health and ICMP errors are described in
[datapath.md](datapath.md#tcp-and-icmp-health-observe-only).

## Network change audit

`mn_rtnl` kprobes `rtnetlink_rcv_msg` and records every state-changing
link / addr / route / neigh / rule / qdisc / filter request together with the
sending process — answering "who changed the routing table?".

- `GET|PUT /api/v1/bpf/rtnl` `{enabled, host_netns_only (true), kinds}`;
  empty `kinds` means everything except class.
- Records: `GET /api/v1/bpf/rtnl/events` (in memory, capped at 2000).
- UI tab **Net changes**.

## Sampled L7 (off by default)

`mn_l7s_ingress/egress` (cgroup_skb on the root cgroup;
`MACHINA_BPF_L7S_CGROUP` overrides) copy the head of at most one payload
segment per flow and direction every `flow_gap_ms` (250), under a host-wide
`rate`, for:

| Protocol | Default port |
|---|---|
| Redis | 6379 |
| PostgreSQL | 5432 |
| MySQL | 3306 |
| Kafka | 9092 |
| HTTP/2 + gRPC | 50051 |

bpfd keeps only the verb or gRPC method path and drops the payload. Configure
`ports: [{port, proto}]` with `GET|PUT /api/v1/bpf/l7-sample`. UI tab
**L7 sampling**.

## VM runtime intelligence (opt-in)

Tracepoints (kvm_exit, scheduler wakeup/switch/migrate, block queue/complete,
page fault and reclaim, IRQ) gated by tracking maps that follow libvirt's
`machine-qemu*` scopes, plus `extra: [{name, cgroup|pid}]`.

`features` selects `flight`, `io`, `mem`, `topology` (empty = all) and builds
per-VM histograms:

- KVM exit reasons, first KVM entry
- vCPU run-queue latency and migrations
- block and vhost latency
- fault and reclaim latency
- per-CPU IRQ time

`GET|PUT /api/v1/bpf/vm-intel`; per-VM report
`GET /api/v1/bpf/vm-intel/vms/{name}`. UI tab **VM runtime**. Smoke:
`scripts/bpf/vmintel-smoke.sh`.

## Workload attribution

Records carry `workload: {kind: vm|pod|container|service, ns?, name}` — see
[datapath.md](datapath.md#workload-attribution).
