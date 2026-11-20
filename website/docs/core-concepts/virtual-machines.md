---
sidebar_position: 2
title: Virtual machines
description: Create, configure, snapshot, back up and migrate KVM guests.
---

# Virtual machines

Every VM operation is available from the web UI, the REST API (`/api/v1/vms`), `machinactl` and the Terraform
provider, all over the same model.

## Create

**New VM** (top bar) walks through name, CPU and memory, disk source and network:

- **Disk sources**: an ISO, a cloud image with cloud-init, a golden image built by the image pipeline (Packer, mkosi
  or virt-builder), or an existing volume.
- **Cloud-init**: user, SSH keys, packages and user-data are injected on first boot.
- **Windows**: Windows 10/11 golden images through dockur when `[libvirt] dockur_windows_allowed = true`.
- **FluxVM**: Create VM → FluxVM for QEMU / Cloud Hypervisor / Firecracker / flux-vm guests (Secure Boot, TPM,
  balloon, install ISOs). See [FluxVM backend](fluxvm.md).
- **Atlas storage**: put the root disk on a Ceph RBD, NFS or ZFS volume when the Atlas integration is enabled.

## Operate

| Task | Where |
| --- | --- |
| Start, shut down, reboot, force off | VM list row actions or VM detail header |
| Console | VM detail leads with a live console; noVNC, SPICE, serial and SSH ([Consoles](consoles.md)). FluxVM QEMU uses **Console (VNC)** |
| Change vCPUs, memory, balloon, boot order | VM detail actions (FluxVM: **Manage** for hot-add and balloon) |
| Disks and CD-ROMs | VM detail → Disks (attach, detach, resize, insert ISO). FluxVM: attach/resize/detach and ISO eject under **Manage** |
| Networks | VM detail → Network (NICs, bridges, nwfilters) |
| Devices | VM detail → Devices (GPU and PCI passthrough, USB) |
| Snapshots | VM detail → Snapshots (create, revert, delete) |
| Guest policy | VM detail → Guest policy ([per-container eBPF policy](../networking/guest-policy.md)) |
| Raw definition | VM detail → XML |

## Protect

- **Snapshots** for quick rollback.
- **Backups**: on demand or nightly with `machinactl backup enable`; see
  [Upgrade and backup](../operations/upgrade-backup.md).
- **HA**: mark a VM HA-protected on the controller and it is restarted elsewhere when its host fails
  ([Controller HA](controller-ha.md)).

## Move

- **Live migration** between hosts in a fleet, from VM detail or the API (a `vm.migrate` task). FluxVM VMs move
  the same way when they are QEMU VMs on a shared disk ([FluxVM](fluxvm.md)).
- **Clone** a VM or turn it into a template (FluxVM: clone needs the guest stopped).
- **Import** from other platforms with hyper2kvm, checked offline by GuestKit.

## Guest agent

With the QEMU guest agent installed, Machina shows guest IPs, filesystems and health, and can freeze filesystems for
consistent snapshots. GuestKit adds deeper diagnostics and per-container policy.
