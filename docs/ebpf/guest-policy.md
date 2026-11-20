# Guest per-container policy (GuestKit)

Back to [native eBPF overview](README.md).

bpfd protects the hypervisor and VM edges. Inside a guest, per-container policy
is enforced by **GuestKit**'s agent (`guestkitd`), which embeds its own eBPF
programs (`crates/guestkit-ebpf` in the guestkit repository). Machina relays
requests to it through the QEMU guest agent; no network path into the guest is
needed.

## What the guest enforces

| Area | Programs | Rules |
|---|---|---|
| Network | `gk_np_egress` / `gk_np_ingress` (cgroup_skb on the container cgroup) | Peer CIDR plus optional protocol/port allow rules, per direction |
| MAC (BPF-LSM) | `gk_lsm_exec` / `gk_lsm_mprotect` / `gk_lsm_open` | Exec allowlist, W+X mappings, device allowlist, writes restricted to allowlisted filesystems |

Rules are keyed by cgroup id. A target is given as a `container` (resolved
through Docker, Podman or crictl to its cgroup subtree) or a raw `cgroup`.

## Safety

- Disabled unless the guest opts in: `capabilities.ebpf: true` in
  `/etc/guestkit/agent-policy.yaml`.
- `mode`: `audit` (default), `enforce` or `off`.
- Enforce needs a 1–3600 s lease whose deadline lives in the policy map, so the
  guest kernel reverts by itself; nothing is persisted.

## API

| Route | Guest RPC |
|---|---|
| `GET /api/v1/vms/{name}/guest-policy` | `guestkit.netpolicy.status` |
| `PUT /api/v1/vms/{name}/guest-policy` (admin) | `guestkit.netpolicy.apply` |
| `GET /api/v1/vms/{name}/guest-lsm` | `guestkit.lsm.status` |
| `PUT /api/v1/vms/{name}/guest-lsm` (admin) | `guestkit.lsm.apply` |

The daemon runs `guestkitctl --json call` through QGA guest-exec, allowlisted
to those four methods.

Example: isolate one container's egress in audit, allowing only PostgreSQL on
the internal network:

```json
{
  "container": "web",
  "mode": "audit",
  "egress": true,
  "ingress": false,
  "rules": [{"direction": "egress", "cidr": "10.0.0.0/8", "proto": "tcp", "port": 5432}]
}
```

The LSM body (`/guest-lsm`) takes `deny_exec` + `allow_exec`, `deny_wx`,
`restrict_devices` + `allow_devices`, and `restrict_writes` + `writable_paths`,
with the same `container` / `cgroup`, `mode` and `lease_secs`.

## UI

VM detail → **Guest policy** tab: pick a container, review status and audited
hits, apply rules in audit, then enforce with a lease.

## Requirements

- QEMU guest agent running in the VM.
- GuestKit (`guestkitd` + `guestkitctl`) installed in the guest, with
  `capabilities.ebpf` enabled.
- Guest kernel with BTF and cgroup v2; LSM rules need `bpf` in the guest's
  active LSM list.

Guest smoke (from the guestkit repository):
`sudo GK_BIN=target/debug ./scripts/ebpf-policy-smoke.sh`.
