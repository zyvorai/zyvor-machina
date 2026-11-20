# Fast path: QUIC-LB, AF_XDP, sched_ext

Back to [native eBPF overview](README.md).

## QUIC-LB

`mn_xdp_quiclb` is tail-called from the uplink XDP dispatcher (`XDP_F_QUICLB`,
before NodePort). `PUT /api/v1/bpf/quic-lb`:

```json
{
  "iface": "eno1", "vip": "203.0.113.10", "port": 443,
  "backends": [{"addr": "10.0.0.11", "server_id": "0a0b0c"},
               {"addr": "10.0.0.12"}],
  "cid_len": 8, "config_id": 0, "mode": "dsr", "enabled": true
}
```

- Short-header packets route by the server id encoded in the QUIC-LB
  connection ID (`cid_len` 3–20, `config_id` 0–6), so a connection keeps its
  backend across client address changes.
- Initial packets route by Maglev (shared with the CNI service LB).
- Delivery is a DSR MAC rewrite or IPIP encapsulation (`encap_src`) followed by
  `XDP_TX`. Backend MACs default to the uplink's ARP entries.
- UI tab **QUIC LB**. Smoke: `scripts/bpf/quiclb-smoke.sh` (veths; sets
  `MACHINA_BPF_XDP_SKB=1` because native veth `XDP_TX` loses frames without NAPI
  on the peer).

## AF_XDP

`PUT /api/v1/bpf/afxdp` `{iface, enabled}` attaches `mn_xdp_afxdp` (an XSKMAP
plus a per-queue gate, up to 64 queues). It refuses the default-route and
uplink interfaces.

A consumer opens its own XSK socket and hands the fd to bpfd with
`BpfdClient::register_xsk(iface, queue, fd)` (SCM_RIGHTS over the bpfd socket).
bpfd drops its copy after inserting it, so when the consumer exits the kernel
clears the entry and frames fall back to the normal stack (`no_socket`
counter).

bpfd's systemd unit leaves `AF_XDP` out of `RestrictAddressFamilies` on
purpose: bpfd never opens XSK sockets itself, so the `xsk` feature probe falls
back to looking for `xsk_map_ops` in `/proc/kallsyms`. Consumers run outside
that unit and open their sockets normally. See
[troubleshooting](../handbook/troubleshooting.md#native-ebpf).

UI tab **AF_XDP**. Smoke: `scripts/bpf/afxdp-smoke.sh` (Python XSK consumer on a
veth).

## sched_ext VM scheduler

`bpf/machina-scx` is a sched_ext struct_ops scheduler (C + libbpf-rs). bpfd
supervises the `machina-scx` helper (installed next to `machina-bpfd`, or
`MACHINA_SCX_BIN`) over a stdin/stdout JSON protocol.

`PUT /api/v1/bpf/scx`:

```json
{"enabled": true, "lease_secs": 600, "vms": ["db-1"], "extra": [],
 "latency_target_us": 500}
```

- Moves the targets' `CPU n/KVM` threads to `SCHED_EXT`. This is a partial
  switch: everything else stays on the fair class.
- Reports per-VM enqueues, dispatches and queue delay.
- `lease_secs` (1–3600) is required. Lease expiry, helper exit or a kernel
  ejection restores `SCHED_OTHER` and stops the helper.
- Needs a kernel with sched_ext (`features.sched_ext`, typically 6.12+). Build
  with `cargo build --release -p machina-scx --features scx` (clang, bpftool and
  `libelf-dev` required).
- UI tab **Scheduler**. Smoke: `scripts/bpf/scx-smoke.sh`.
