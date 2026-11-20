---
sidebar_position: 2.5
title: FluxVM backend
description: Second VM backend next to libvirt — QEMU VNC, disks, snapshots, migration and fleet HA.
---

# FluxVM backend

[FluxVM](https://github.com/zyvorai/zyvor-fluxvm) is a second VM backend next to libvirt. A host runs
`fluxvm-api` (default `http://127.0.0.1:7788`); Machina merges those VMs into the same list, UI and API as
libvirt guests (`backend: "fluxvm"`).

Full operator guide (ISO installs, migrate/HA, regression):
[docs/fluxvm.md](https://github.com/zyvorai/zyvor-machina/blob/main/docs/fluxvm.md).

Hands-on (create → VNC → disk → snapshot → backup → clone):
[FluxVM day two](../getting-started/fluxvm-day-two.md).

## Enable

Daemon (`/etc/machina/config.toml`):

```toml
[fluxvm]
enabled = true
base_url = "http://127.0.0.1:7788"
# token / token_file, insecure_tls, default_backend
```

Agent (fleet): `MACHINA_FLUXVM_URL`, optional `MACHINA_FLUXVM_TOKEN`, `MACHINA_FLUXVM_INSECURE_TLS=1`.
Check with `GET /api/v1/fluxvm/status`.

## Create

**Create VM → FluxVM**, or `POST /api/v1/vms` with `backend: "fluxvm"`:

| Field | Meaning |
| --- | --- |
| `fluxvm_backend` | `qemu`, `cloud-hypervisor`, `firecracker`, `flux-vm` or `auto` |
| `fluxvm_image` | Disk image on the host |
| `fluxvm_shared_disk` | In-place shared disk (needed for live migration and HA) |
| `fluxvm_isos` | Install ISOs (QEMU, at most 4) |
| `secure_boot` / `vtpm` | UEFI Secure Boot and emulated TPM 2.0 (QEMU; Windows 11) |
| `fluxvm_firmware` | Optional per-VM OVMF path |
| `fluxvm_balloon` | Virtio balloon at create (QEMU) |
| Network | eBPF namespace (default), host bridge, direct uplink, or `user` / `none` |

## Day-2 (daemon)

All VM routes take `?backend=fluxvm` (a name that is not a libvirt domain falls back to FluxVM).

| Area | Notes |
| --- | --- |
| Power, metrics | Every engine |
| **Console (VNC)** | QEMU: `/ws/v1/vnc/{name}?backend=fluxvm` via FluxVM `ws-ticket` (no host `socat`) |
| Serial / agent console | Serial interactive on QEMU; agent shell on every engine |
| Snapshots / backups | Every engine; live backup QEMU + default storage |
| Hot-add vCPU / memory | QEMU, Cloud Hypervisor (add-only) |
| Extra NICs, data disks, balloon, install ISO eject | QEMU |
| Clone / rename / logs | Clone needs the VM stopped |
| Live migration | QEMU on a shared disk; eject ISOs and restart after hot-add first |

UI: VM detail → **Manage** for hotplug, disks, media, snaps, backups and migrate; **Console (VNC)** for installers.

## Fleet

With the agent pointed at fluxvm-api, the controller keeps `inventory_source = 'fluxvm'` rows:

- `vm.power` / `vm.delete`
- `vm.migrate` (+ DRS) and HA re-create (`ha.recover`, `POST /api/v1/vms/{id}/fluxvm/recover`)
- `vm.snapshot*` / `vm.backup*` through the same agent RPCs (FluxVM resolved by name)

Discovered FluxVM VMs stay unmanaged unless you mark them managed.

## Engine matrix

| Feature | QEMU | Cloud Hypervisor | Firecracker | flux-vm |
| --- | --- | --- | --- | --- |
| Power, metrics, snapshots | yes | yes | yes | yes |
| Graphical VNC | yes | no | no | no |
| Serial | interactive | read-only log | read-only log | read-only log |
| Agent console | yes | yes | yes | yes |
| Hot-add / NICs / disks / balloon / ISOs | yes (hot-add also CH) | partial | no | no |
| Live migration / HA re-create | shared disk | no | no | no |

## Limits (FluxVM-side)

- CD-ROM media only at create; eject keeps an empty drive (no re-insert).
- Cross-host live migrate needs matching QEMU machine types (FluxVM pins this; mixed QEMU versions can fail).
- Backups need default or shared storage (not LVM thin / NBD / Ceph RBD through FluxVM).

Claims: [C72](https://github.com/zyvorai/zyvor-machina/blob/main/docs/claims.md), [C73](https://github.com/zyvorai/zyvor-machina/blob/main/docs/claims.md).
Regression: `make regression-fluxvm`. Tutorial: [FluxVM day two](../getting-started/fluxvm-day-two.md).
