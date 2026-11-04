---
sidebar_position: 8
title: Integrations
description: Storage, network security, containers, Kubernetes and automation integrations.
---

# Integrations

Integrations are enabled with environment variables on the controller. Atlas is off by default.

## Atlas storage

Puts VM disks on Ceph RBD, NFS or ZFS volumes managed by the Atlas storage control plane, with snapshot, backup and
restore.

```bash
ATLAS_ENABLED=1
ATLAS_BASE_URL=http://127.0.0.1:5110
ATLAS_TOKEN=<service-account JWT>
```

Create a VM on Atlas storage by passing `atlas_root_disk: true` to `POST /api/v1/vms`.

## GuestKit

GuestKit's in-guest agent reports guest health, checks VMs offline before migration, and enforces per-container eBPF
policy inside guests. Machina talks to it through the QEMU guest agent; see [Guest policy](../networking/guest-policy.md).
Controller-side features are on by default (`GUESTKIT_ENABLED=true`).

## Networking and security

Network enforcement and observability are built in, not integrated: Machina's own eBPF service, `machina-bpfd`,
replaces Cilium, Tetragon, Netra and PacketWolf, and Machina no longer talks to any of them. See
[Native eBPF](../networking/ebpf-overview.md).

## FluxVM

A second VM backend next to libvirt: VMs on QEMU, Cloud Hypervisor, Firecracker or flux-vm from a host's
`fluxvm-api` appear with `backend: "fluxvm"`. Enable it on the daemon with `[fluxvm] enabled = true` and on each
host's agent with `MACHINA_FLUXVM_URL`. Day-2 covers QEMU VNC (noVNC via `ws-ticket`), serial and agent consoles,
snapshots, backups, hot-add, data disks, clone/rename, balloon, Secure Boot/TPM at create, install ISOs, live
migration and HA re-create (QEMU on a shared disk). See [FluxVM backend](fluxvm.md) and the full
[operator guide](https://github.com/zyvorai/zyvor-machina/blob/main/docs/fluxvm.md).

## Containers and Kubernetes

- **Containers.** Local Podman or Docker containers and Podman pods run next to your VMs (configure the socket in
  `[vessel]`).
- **KubeVirt.** Inventory of KubeVirt VMs and a documented migration path.

## Migration into KVM

hyper2kvm converts guests from other platforms into KVM; GuestKit checks them offline before cutover.

## Automation

- [Terraform provider](https://github.com/zyvorai/zyvor-machina/blob/main/terraform/machina/README.md)
- [TypeScript SDK](https://github.com/zyvorai/zyvor-machina/blob/main/sdk/typescript/README.md)
- OpenAPI spec and the `machinactl` CLI
