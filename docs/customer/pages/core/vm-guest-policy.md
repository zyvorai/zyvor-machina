# VM detail: Guest policy

## Purpose

Restrict what individual containers **inside a VM** may do: which networks they reach, which programs they run, which devices they open and where they write. The rules are enforced inside the guest by GuestKit's agent (`guestkitd`) with its own eBPF programs. Machina sends them through the QEMU guest agent, so no network path into the guest is needed.

## When to use it

- A VM runs Docker, Podman or Kubernetes workloads and one container should only reach, say, its database
- You want to block unexpected binaries, writable+executable memory, or writes outside a data directory for one container
- You need evidence first: audit mode reports what *would* be blocked

## How to get there

- Open a VM (**Virtual Machines → VM name**) and pick the **Guest policy** tab.
- The VM must be running.

## Before you start

- QEMU guest agent running in the VM.
- GuestKit (`guestkitd` and `guestkitctl`) installed in the guest, with
  `capabilities.ebpf: true` in `/etc/guestkit/agent-policy.yaml`. Without the
  opt-in the tab reports that guest eBPF is disabled.
- Guest kernel with BTF and cgroup v2. The MAC section also needs `bpf` in the
  guest's active LSM list.
- Viewing needs any role; applying needs **Admin**.

## What you can do

The tab has two sections. Each one lists the containers that already have
rules, with their mode and audited hits, and a form to apply new rules.

**Container network policy**

1. Enter the container name or id.
2. Choose which directions to police (egress, ingress).
3. Add allow rules, one per line, for example `egress 10.0.0.0/8 tcp 5432` or `egress 0.0.0.0/0 udp 53`.
4. Apply in **Audit (report only)**. Watch the hit counters.

**Container MAC (BPF-LSM)**

- **Deny exec** with an allowlist (`/usr/bin/python3, /app/server`)
- **Deny W+X** memory mappings
- **Restrict devices** with an allowlist (`c 10:200, b 8:*`)
- **Restrict writes** to allowlisted paths (`/tmp, /data`)

## Audit, then enforce

- **Audit** is the default and only reports violations.
- **Enforce (leased)** denies them for the lease you enter (1–3600 s). The
  deadline lives in the guest's policy map, so the guest kernel reverts to
  audit by itself when it expires, even if Machina is unreachable.
- Nothing is persisted. A guest reboot or `guestkitd` restart clears the rules.

## If something is wrong

- **Guest agent not connected:** start `qemu-guest-agent` in the VM.
- **eBPF disabled in guest:** set `capabilities.ebpf: true` in the guest's agent policy and restart `guestkitd`.
- **Container not found:** use the name or id shown by `docker ps` / `podman ps` / `crictl ps` in the guest.

## Related pages

- [Virtual Machines](vms.md)
- [Native eBPF](../platform-security/platform-zyra-security-native-bpf.md)
- [Page index](../../PAGE_INDEX.md)
