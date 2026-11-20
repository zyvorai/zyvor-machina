# Enforcement: policies, VM edge, QEMU sandbox, VMM guard, direct redirect

Back to [native eBPF overview](README.md). Every feature here follows the
[safety model](README.md#safety-model): observe or audit first, drop only under
a live lease, nothing persisted.

## Policies and the enforcement lease

Policies are created on one host with `POST /api/v1/bpf/policies` or fleet-wide
through the controller (`/api/v1/zeus-security/enforcement/*`). Controller-owned
policies are prefixed `ctl-` and scoped `fleet`, `host:<id>`, `vm:<name>` or
`cgroup:<path>`; the controller pushes them to each host's agent
(`BpfSyncPolicies`).

| Kind | Effect |
|---|---|
| `deny_ip` | Drop traffic to/from an IP or CIDR |
| `deny_port` | Drop a protocol/port |
| `tc_allow` | Default-deny with an allowlist (never on an uplink) |
| `allow_port` | Allowlist entry for a port |
| `deny_process` | Block exec of a binary |
| `deny_file` | Block opening a path |
| `deny_cap` | Block a capability |
| `deny_dns` | Refuse resolution of a name |
| `rate_limit` | Token bucket on new workload connections per tap (`100/s`, `600/m`, `50/s burst 200`; VM taps only) |

Invalid matches are rejected with HTTP 400 `enforcement_rejected` before
anything is stored.

Mode is set with `PUT /api/v1/bpf/mode` (`observe` or `enforce`). Enforce needs
a lease (`MACHINA_BPF_ENFORCE_LEASE_SECS`, default 900 s) whose deadline is
checked in the datapath. A lapsed lease or a bpfd restart means observe. The
lease covers policies, the shield, VM edge, the QEMU sandbox and node
isolation.

## VM edge

`PUT /api/v1/bpf/vm-edge` describes VMs and group policy:

```json
{
  "vms": [{"name": "web-1", "group": "web", "addresses": ["10.0.0.5"],
           "isolate_ingress": true, "isolate_egress": false,
           "egress_mbps": 500, "ingress_mbps": 500, "pps": 50000}],
  "policy": [{"group": "web", "peer": "world", "egress": false,
              "proto": "tcp", "port": 443}]
}
```

bpfd chains `mn_vm_edge_in/out` (TCX) on each VM's taps (found through libvirt
unless `taps` is given) with group identities, conntrack reply bypass and
per-direction Mbit/s + PPS token buckets. `peer` is `any`, `world` or another
group. Rate limits always apply; isolation misses drop only under the lease,
otherwise they count as `observed`. The daemon refreshes bpfd on VM
start/stop/delete. UI tab **VM Edge**.

## QEMU sandbox

`PUT /api/v1/bpf/vm-sandbox` confines QEMU itself:

- `mn_qemu_device`: cgroup device allowlist — libvirt's default nodes plus
  kvm, vhost, tun, vfio and pts, plus `extra_devices` such as `c 10:232 rw`.
- `mn_qemu_egress`: QEMU's own sockets may reach loopback and `egress_ports`
  (default migration `49152-49215` and NBD `10809`).
- `auto: true` sandboxes every `machine.slice/machine-qemu*` scope;
  `POST` / `DELETE /api/v1/bpf/vm-sandbox/{vm}` pins a single VM.

Both programs are bpf_link multi-attach (AND-ed with libvirt's own device
program), record violations per cgroup, and deny only with `mode: enforce` and
a live lease.

## VMM guard (BPF-LSM)

`PUT /api/v1/bpf/guard`:

```json
{"enabled": true, "mode": "audit", "exec": true, "wx": true, "devices": true,
 "allow_exec": [], "allow_devices": [], "lease_secs": 600}
```

LSM hooks (`bprm_check_security`, `file_mprotect`, `file_open`) scoped to QEMU
cgroups flag exec outside QEMU's binaries and `allow_exec`, W+X mappings, and
character-device opens outside libvirt's defaults and `allow_devices`. Events:
`GET /api/v1/bpf/guard/events`.

- Audit by default. Enforce needs `bpf` in `/sys/kernel/security/lsm`;
  otherwise status reports `lsm_inactive` and enforce is refused.
- Enforce carries its own lease (1–3600 s) checked inside the hook.
- Struct offsets come from vmlinux BTF.
- UI tab **VMM guard**. Smoke: `scripts/bpf/guard-smoke.sh`.

## Direct tap redirect

`PUT /api/v1/bpf/direct` (opt-in per VM):

```json
{"vm": "web-1", "outer_iface": "veth-up", "enabled": true, "force": false,
 "ips": [], "reverse": true}
```

Frames for the guest MAC (or `ips`) arriving on `outer_iface` go straight into
the VM tap and, with `reverse`, the VM's frames go straight out, bypassing the
bridge. Refuses a physical NIC without `force`; entries stay idle unless the
enforcement lease is live. UI tab **Direct redirect**.

## Guests

Policy inside a guest (per container) is enforced by GuestKit, not bpfd: see
[guest-policy.md](guest-policy.md).
