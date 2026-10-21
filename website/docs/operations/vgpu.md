---
sidebar_position: 8
title: vGPU partitions
description: Split a GPU into mediated-device partitions and give each to a VM, instead of passing the whole card through.
---

# vGPU partitions

PCI passthrough gives one VM a whole GPU. If the GPU's driver supports **mediated devices** (mdev), the card can be split into
partitions and each partition given to a different VM. Machina lists what the host offers, creates and removes partitions, and
attaches them to VMs as `type='mdev'` host devices.

Whether a host can do this depends on its driver and licence, not on Machina: an NVIDIA vGPU host driver (licensed by NVIDIA) or
Intel GVT-g exposes `/sys/class/mdev_bus`. A GPU bound to `vfio-pci` for passthrough does not.

## Use it

All calls need the same permission as PCI passthrough (`vms:write` plus the USB/PCI privilege).

```bash
# What can this host carve out, and what exists?
curl -sk -H "Authorization: Bearer $TOKEN" https://HOST:5092/api/v1/host/mdev
# {"types":[{"parent":"0000:03:00.0","type_id":"nvidia-222","name":"GRID A16-4C","available_instances":3,...}],
#  "instances":[]}

# Create a partition (a uuid is generated when you omit it)
curl -sk -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"parent":"0000:03:00.0","type":"nvidia-222"}' https://HOST:5092/api/v1/host/mdev

# Give it to a VM, and take it back
curl -sk -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"uuid":"<uuid>"}' https://HOST:5092/api/v1/vms/web-01/hostdev/mdev/attach
curl -sk -X POST ... /api/v1/vms/web-01/hostdev/mdev/detach

# Remove it once no VM uses it
curl -sk -X DELETE -H "Authorization: Bearer $TOKEN" https://HOST:5092/api/v1/host/mdev/<uuid>
```

Machina creates and removes the partitions itself and attaches them with `managed='no'`, so libvirt never creates or deletes one.
Partitions do not survive a host reboot: recreate them (for example from a boot script that calls the same API) before starting
VMs that reference them.

## Limits

- **Daemon and agent only.** The host and per-VM calls exist on the daemon (`/api/v1/host/mdev`, `/vms/{name}/hostdev/mdev/*`)
  and the agent can attach and detach (`mdev.attach`, `mdev.detach`). There is no controller API, scheduler awareness (a VM with
  a partition is not pinned to its host by the placement engine) or web page yet.
- **No live migration** of a VM holding a partition; detach first.
- Partition availability is whatever the driver reports at that moment; creating a type with no instances left is refused.
- Not yet run on a vGPU host: see claims C59.
