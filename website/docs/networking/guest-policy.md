---
sidebar_position: 5
title: Guest policy
description: Per-container network and LSM policy inside VMs, enforced by GuestKit and managed from Machina.
---

# Guest policy

Machina can enforce eBPF policy **inside a VM**, per container, without any
network path into the guest. Requests travel through the QEMU guest agent to
GuestKit's `guestkitd`, which loads its own eBPF programs in the guest kernel.

## Network policy

Isolate a container's egress and/or ingress and allow specific peers:

```text
egress 10.0.0.0/8 tcp 5432
egress 0.0.0.0/0 udp 53
```

## LSM policy

- Allow exec only of listed binaries.
- Block W+X memory mappings.
- Allow only listed devices.
- Restrict writes to listed paths.

## Safety

- Off unless the guest opts in (`capabilities.ebpf: true` in
  `/etc/guestkit/agent-policy.yaml`).
- Audit by default; enforce needs a lease (up to one hour) held in the guest
  kernel, which reverts by itself.

## Where

- UI: VM detail → **Guest policy** tab.
- API: `GET|PUT /api/v1/vms/{name}/guest-policy` and `/guest-lsm` (PUT is
  admin-only).

Requirements: QEMU guest agent and GuestKit in the guest, a kernel with BTF and
cgroup v2, and BPF-LSM for the LSM rules.
