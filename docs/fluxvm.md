# FluxVM backend

[FluxVM](https://github.com/zyvorai/zyvor-fluxvm) is a second VM backend next to libvirt. A host runs
`fluxvm-api` (default `http://127.0.0.1:7788`), which starts VMs on QEMU, Cloud Hypervisor, Firecracker or
flux-vm. Machina shows these VMs next to libvirt VMs and manages them from the same UI, REST API and controller.

- **Daemon** — one host: create, power, consoles, snapshots, backups, hot-add, NICs, install ISOs, live migration.
- **Controller** — the fleet: inventory, power and delete through each host's agent, migration between hosts (and
  DRS), HA re-create on another host.

**Hands-on:** [Tutorial 07 — FluxVM day two](tutorials/07-fluxvm-day-two.md) (VNC, data disk, snapshot, backup, clone).
Site: [FluxVM day two](https://zyvorai.github.io/zyvor-machina/docs/getting-started/fluxvm-day-two).

## Set up

### Daemon (`/etc/machina/config.toml`)

```toml
[fluxvm]
enabled = true
base_url = "http://127.0.0.1:7788"
# token = ""             # bearer token from fluxvm [[auth.tokens]]
# token_file = ""        # or read it from a file
# insecure_tls = false   # accept a self-signed https:// fluxvm-api (also wss:// consoles)
# default_backend = "auto"   # auto | qemu | cloud-hypervisor | firecracker | flux-vm
```

Check it with `GET /api/v1/fluxvm/status` (`enabled`, `reachable`).

### Agent (`/etc/default/machina-platform`)

For the controller to see and manage a host's FluxVM VMs, `machina-agent` on that host needs the fluxvm-api:

| Variable | Default |
| --- | --- |
| `MACHINA_FLUXVM_URL` | the host's `[fluxvm] base_url` when `enabled` |
| `MACHINA_FLUXVM_TOKEN` | `[fluxvm] token` / `token_file` |
| `MACHINA_FLUXVM_INSECURE_TLS` | `[fluxvm] insecure_tls` |

Restart `machina-agent` after changing them. With no fluxvm-api the agent simply reports no FluxVM VMs.

### Upgrading FluxVM

`fluxvm.service` uses `KillMode=process`, so `systemctl restart fluxvm` after installing a new `fluxctl` leaves running
VMs alone; the new daemon picks them up from its state dir. Building FluxVM needs a sibling `guestkit` checkout.

FluxVM 0.4.0 makes its native eBPF VM edge the default: with no `[sandbox.dataplane]` mode in `/etc/fluxvm.toml` it runs
`mode = "ebpf"`, `required = true`, so a VM whose tap can't get the BPF program fails to create or start instead of
falling back to nftables. Before upgrading such a host, run `./scripts/network-fabric-preflight.sh --require-bpf` and
install the BPF objects (`sudo ./scripts/enable-network-fabric-ga.sh --dry-run` shows what it would change; its GA
profile also turns on deny-by-default), or keep the old behaviour with:

```toml
[sandbox.dataplane]
mode = "legacy"
```

Hosts that already set a mode (`cilium`, `legacy`, `ebpf` with `required = false`) behave as before. User-mode NAT and
`none` networking have no VM edge and are unaffected. FluxVM's
[primary eBPF guide](https://github.com/zyvorai/zyvor-fluxvm/blob/main/docs/primary-ebpf.md) has the details.

## Create

`POST /api/v1/vms` with `backend: "fluxvm"` (UI: Create VM → FluxVM):

| Field | Meaning |
| --- | --- |
| `fluxvm_backend` | `qemu`, `cloud-hypervisor`, `firecracker`, `flux-vm` or `auto` |
| `fluxvm_image` | Disk image on the host |
| `fluxvm_kernel`, `fluxvm_initrd`, `fluxvm_kernel_args` | Direct kernel boot (Firecracker and flux-vm need a kernel; QEMU takes a bzImage) |
| `fluxvm_agent: false` | Turn off the in-guest agent (on by default; needed for the agent console) |
| `fluxvm_shared_disk: true` | Use the image in place as a shared disk (never cloned or deleted, guarded by `<disk>.fluxvm-lock`). Needed for migration and HA re-create. Not for flux-vm |
| `fluxvm_isos` | Install ISOs on the host (QEMU, at most 4), attached read-only as SATA CD-ROMs `install`, `cd2`, `cd3`, `cd4`. `auto` picks QEMU. For a fresh install, point `fluxvm_image` at an empty raw file |
| `secure_boot` / `vtpm` | QEMU Secure Boot and emulated TPM 2.0 (Windows 11) |
| `fluxvm_firmware` | Optional per-VM OVMF path; empty = host `qemu_ovmf_code` |
| `fluxvm_balloon` | QEMU virtio balloon at create |
| `fluxvm_bridge` | Tap on an existing host bridge |
| `fluxvm_direct_uplink` (+ `fluxvm_direct_mode`, `fluxvm_direct_guest_ips`) | Bridge-less eBPF redirect between a host NIC and the tap |
| `fluxvm_network: "user"` / `"none"` | QEMU user-mode NAT (forces QEMU) / no network |

With none of the network fields, the VM gets a tap in its own network namespace with DHCP and NAT. Every mode except
`user` and `none` gets FluxVM's eBPF VM edge.

## What each engine supports

| Feature | QEMU | Cloud Hypervisor | Firecracker | flux-vm |
| --- | --- | --- | --- | --- |
| Power, metrics, snapshots | yes | yes | yes | yes |
| Graphical VNC | yes | no | no | no |
| Serial console | interactive | read-only log | read-only log | read-only log |
| Agent console (guest shell) | yes | yes | yes | yes |
| Hot-add vCPU / memory | yes | yes | no | no |
| Extra NICs / data disks / balloon | yes | no | no | no |
| Install ISOs (CD-ROM) | yes | no | no | no |
| Backups (default or shared storage) | running or stopped | stopped | stopped | stopped |
| Live migration, HA re-create | shared disk | no | no | no |

The VM detail page shows only what the VM's engine supports (**Manage** and **Agent console** tabs).

## Install an OS from an ISO

Use this for guests that don't come as a ready disk image: Windows, BSDs, or a Linux installer. QEMU only.

### How the drives work

- `fluxvm_isos` takes up to four ISO paths. Machina names the drives in order: `install`, `cd2`, `cd3`, `cd4`.
  Blank entries are skipped.
- FluxVM attaches each one read-only as a SATA CD-ROM on q35's AHCI controller, so installers see it without extra
  drivers.
- No boot order is forced. Firmware skips a blank root disk and boots the first CD-ROM; once an OS is installed, it
  boots from the disk. If the root disk already has a bootloader, the ISO is never booted.
- Each path must be a file under FluxVM's `policy.allowed_image_dirs` (when that's set, in `/etc/fluxvm.toml`) or a
  block device under `/dev`. Symlinks are resolved first.
- Ejecting removes the medium but keeps the drive, so the guest's device layout (and Windows drive letters) doesn't
  change across restarts and migrations. A drive can't be refilled, either live or later; recreate the VM to use a
  different ISO.

### Steps

1. **Put the ISOs and a blank disk on the host.** For a VM you'll migrate or protect with HA, use a raw file on
   storage every host can reach:

   ```bash
   sudo cp win2022.iso virtio-win.iso /var/lib/fluxvm/images/
   sudo truncate -s 80G /var/lib/fluxvm/shared/win01.raw   # sparse, used in place
   ```

   For a VM on FluxVM's default storage, point the image at an empty raw or qcow2 file instead and set `disk_gb`;
   FluxVM gives the VM its own copy at that size.

2. **Create the VM.** UI: **Create VM → FluxVM**. Pick `qemu` (or `auto`), put the blank disk in the image field
   (tick the shared-disk box for the in-place file) and list the ISOs in **Install ISOs**, one per line. Leave kernel
   and initrd empty. Turn the guest agent off unless the installed OS will run `fluxvm-agent`. Or with the API:

   ```bash
   curl -sk -b cookies -X POST https://HOST:5092/api/v1/vms -H 'Content-Type: application/json' -d '{
     "name": "win01", "backend": "fluxvm", "fluxvm_backend": "qemu",
     "vcpus": 4, "memory_mb": 8192, "disk_gb": 0,
     "fluxvm_image": "/var/lib/fluxvm/shared/win01.raw", "fluxvm_shared_disk": true,
     "fluxvm_isos": ["/var/lib/fluxvm/images/win2022.iso", "/var/lib/fluxvm/images/virtio-win.iso"],
     "fluxvm_agent": false
   }'
   ```

   `fluxvm_isos` with `fluxvm_backend: "auto"` picks QEMU. Any other engine, or more than four ISOs, is a 400.

3. **Open the installer's screen.** Use VM detail → **Console (VNC)** (or the Console button). Machina mints a
   FluxVM `ws-ticket` (`kind: vnc`) and proxies noVNC to the QEMU display — no host `socat` needed. The same
   WebSocket is at `/ws/v1/vnc/{name}?backend=fluxvm&token=…`. Linux installers that print to the serial console
   can use the **Serial** tab instead.

4. **Install.** The root disk is virtio-blk and the NIC virtio-net. Windows Setup lists no disk until you load the
   storage driver: **Load driver → `cd2` → `viostor\<version>\amd64`**. Install `NetKVM` from the same ISO for the
   network afterwards.

5. **Eject the ISOs.** VM detail → **Manage → Install media** lists the drives, each with its ISO or `empty`. Click
   **Eject** on each loaded drive. It works while the VM runs. Or:

   ```bash
   curl -sk -b cookies -X POST 'https://HOST:5092/api/v1/vms/win01/cdrom/eject/install?backend=fluxvm'
   # {"status":"ejected","name":"win01","target":"install","backend":"fluxvm"}
   ```

   Ejecting an empty drive succeeds and changes nothing; an unknown drive name is a 400 (`cdrom "x" not found`).

### What a loaded drive blocks

| Path | With an ISO in a drive | After eject |
| --- | --- | --- |
| Daemon live migration (`POST …/migrate`) | 400 `… eject cdrom "install" first (POST /v1/vms/{id}/cdroms/{name}/eject)` | Works; the receiver keeps the empty drive |
| **Manage → Migrate** button | Disabled, with "Eject install first" | Enabled |
| Controller pre-check `fluxvm_mobility` | Fails: `CD-ROM 'install' still holds install media; eject it before migrating` | Passes |
| Controller `vm.migrate` / DRS | Refused with the same reason | Works |
| HA re-create (`ha.recover`, `…/fluxvm/recover`) | Re-created with the ISO, which must exist at the same path on the target host | Re-created without the ejected drives (FluxVM won't create an empty drive) |
| Restart | Boots with the ISO still attached | Boots with an empty drive, no ISO |

An ISO path is host-local even when the disk is shared, which is why migration refuses it.

### Where the drives show up

`GET /api/v1/vms/{name}?backend=fluxvm` lists each drive in `disks` with `device: "cdrom"`, `target` = drive name,
`source` = ISO path (`""` once ejected), `bus: "sata"` and `readonly: true`:

```json
{"device": "cdrom", "source": "", "driver": "raw", "target": "install", "bus": "sata", "readonly": true, "shareable": false}
```

## Operate (daemon)

All VM routes take `?backend=fluxvm` (a name that isn't a libvirt domain falls back to FluxVM).

- **Snapshots** — `GET|POST /api/v1/vms/{name}/snapshots`, `DELETE …/snapshots/{snap}`, `POST …/snapshots/{snap}/revert`.
  A stopped VM relaunches from the snapshot; a running QEMU VM must be stopped first.
- **Backups** — `POST /api/v1/backups {vm_name, backend: "fluxvm", compress}`, list with
  `GET /api/v1/backups?backend=fluxvm&vm=<name>`, `POST /api/v1/backups/restore {backup_id, backend: "fluxvm", vm_name}`
  (VM stopped), `DELETE /api/v1/backups/{id}?backend=fluxvm`. Works on every engine for VMs on FluxVM's default
  storage or a shared disk. A running VM can be backed up only on QEMU with default storage (through a short internal
  snapshot); stop it first on the other engines. Backups are qcow2; a restore converts back to the disk's own format
  (raw on every engine but QEMU with default storage) and replaces the disk, so internal snapshots taken after the
  backup are gone.
- **Hot-add** — `POST /api/v1/vms/{name}/vcpus/{n}` and `…/memory/{mb}`. Add-only: a smaller value is refused. The
  next start boots at the created size. A hot-added VM must be restarted before it can migrate.
- **Extra NICs** — `POST …/nic/attach {network: "<bridge>"}` returns the NIC's `mac`; `POST …/nic/detach/{mac}`
  removes it. Each NIC is a tap on that host bridge. When the primary NIC is in the VM's own network namespace,
  FluxVM passes the bridge taps to QEMU as open file descriptors, at hot-add and on every restart. A VM with extra
  NICs can't migrate; after removing the last one, restart it first (it still has the PCIe layout it booted with).
- **Install media** — `POST …/cdrom/eject/{drive}` removes an ISO, live on a running VM; the empty drive stays.
  `…/cdrom/insert` is refused (400: FluxVM attaches ISOs only at create). UI: **Manage → Install media**. See
  [Install an OS from an ISO](#install-an-os-from-an-iso).
- **Live migration** — `POST …/migrate {dest_uri, live: true}`. `dest_uri` is `local` (a fresh QEMU process on the
  same host) or another host's fluxvm-api URL with `dest_token`; optional `listen_host`, `advertise_host`,
  `bandwidth_mbps`, `max_downtime_ms`. The disk is never copied. A VM with an ISO still in a drive is refused until
  it's ejected (the **Migrate** button stays disabled).
- **Consoles** — QEMU VNC at `/ws/v1/vnc/{name}?backend=fluxvm&token=…` (FluxVM `ws-ticket`); serial at
  `/ws/v1/console/{name}?backend=fluxvm&token=…`; guest shell at
  `/ws/v1/fluxvm-console/{name}?token=…&cols=&rows=` (needs `vms:write`; keystrokes as binary frames, resize as
  `{"type":"resize","cols":…,"rows":…}` text). Get `token` from `POST /api/v1/ws-token`.
- **Disks** — `POST …/disk/attach` with `target` (FluxVM disk name) and either `size_gb` (blank disk) or `source`
  (host path); `POST …/disk/resize/{target}` (`vda` maps to FluxVM `root`); `POST …/disk/detach/{target}`.
- **Clone / rename** — `POST …/clone {new_name}` (source stopped); `POST …/rename {new_name}`.
- **Balloon** — `POST …/balloon/{mb}` when the VM was created with a balloon device.
- **Logs** — `GET …/logs?backend=fluxvm` (FluxVM console log, not libvirt qemu log).

## Fleet (controller)

The agent adds FluxVM VMs to its inventory, and the controller keeps them as `inventory_source = 'fluxvm'` rows
(migration `068`). Rows are removed only when the agent reached the fluxvm-api, and never during a migration.

- **Power and delete** — `vm.power` / `vm.delete` go to the host's fluxvm-api through its agent.
- **Migration** — `POST /api/v1/vms/{id}/migrate {dest_host_id, live: true}` (and DRS, which uses the same task).
  The controller starts a receiver on the destination agent, starts the migration on the source, waits for it,
  removes the paused source and adopts the receiver. The destination may be the same host. A failure cancels both
  sides. The pre-check (`POST …/migrate/precheck`) asks the source agent for the VM's current state and checks the
  engine, shared disk, hot-add, install media and destination capacity.
- **HA** — when a host fails (and the VM has an HA policy), `ha.recover` re-creates the VM from its last FluxVM
  record on the target host, on the same shared disk, breaking the disk lock (`shared_takeover`). A stopped instance
  with the same name already on the target (left from an earlier move) is removed first. Hot-add doesn't
  block it, and ejected CD-ROM drives are left out of the new VM (FluxVM won't create an empty drive). The same
  re-create on demand: `POST /api/v1/vms/{id}/fluxvm/recover {host_id?}` (operator; default host = current host).
- **Reconcile** — discovered FluxVM VMs are unmanaged, so the reconciler doesn't fight changes made directly in
  FluxVM. Rows marked managed are reconciled like libvirt VMs.

Disks must be on storage every host can reach (NFS or Ceph RBD mounted at the same path) for migration or HA
between hosts.

## Test

```bash
make regression-fluxvm   # scripts/regression/ops-fluxvm.js
```

It creates three throwaway VMs and covers the consoles, snapshots, hot-add, NICs (host bridge, and on a namespace VM
across a restart), backup/restore on QEMU and on a stopped Firecracker VM, an install ISO (migration refused, eject,
re-insert refused), daemon migration, the controller inventory row, pre-check, `vm.migrate` and HA re-create of the
VM with its ejected drive. `FLUXVM_ONLY=b` (or `a,c`) runs a subset. Overrides: `FLUXVM_IMAGE`, `FLUXVM_QEMU_KERNEL`,
`FLUXVM_QEMU_INITRD`, `FLUXVM_FC_KERNEL`, `FLUXVM_BRIDGE` (`virbr0`), `FLUXVM_SHARED_DIR`
(`/var/lib/fluxvm/shared`), `FLUXVM_SKIP_PLATFORM=1`. Results: [regression RESULTS](../scripts/regression/RESULTS.md).

Between two hosts: join the second host's agent to the same controller, mount one share at the same path on both
(for example NFS exported to the second host's address only) and set `FLUXVM_SHARED_DIR` to it,
`FLUXVM_DEST_HOST_ID` to the second host's controller id, `FLUXVM_DEST_SSH=user@host` and, if its fluxvm-api isn't on
`127.0.0.1:7788` there, `FLUXVM_DEST_FLUXVM_URL`. The run then checks the disk is visible on the destination,
live-migrates VM B there (or, if that fails, stops it and re-creates it there with HA), and re-creates it back on the
first host. The destination copy of B is deleted at the end; the shared disk is removed only once B is gone.

The host needs the image and kernels under `/var/lib/fluxvm` (`images/linux-agent.raw`, `kernels/bionic-vmlinuz-4.15`,
`kernels/bionic-initrd-4.15`, `kernels/vmlinux` for Firecracker) and the bridge. On a host where other sessions sync into
the same deploy tree, run from a copy of `scripts/regression` (or set `MACHINA_REGRESSION_OUT`): a `rsync --delete`
removes `results/` mid-run.

## Limits

- HA re-create between two hosts works in both directions (verified 175 ↔ 212 over NFS). It assumes the old host is
  fenced: it doesn't stop the VM there, so stop it first when the old host is still up.
- Live migration between hosts with different QEMU versions fails ("Unable to write to socket: Broken pipe"): FluxVM
  starts both sides with the unversioned `q35` machine, which resolves to `pc-q35-8.2` on QEMU 8.2 and `pc-q35-10.2` on
  QEMU 10.2. Hosts on the same QEMU version aren't affected. The fix is in FluxVM (pin the source's resolved machine
  type on the receiver).
- Backups need FluxVM's default storage or a shared disk file (not LVM thin, NBD or Ceph RBD). Only QEMU with
  default storage can back up a running VM.
- Hot-add is per engine (table above); extra NICs are QEMU-only.
- Migration needs QEMU on a shared disk, no extra NICs, no install media in a CD-ROM, and no hot-add or NIC removal
  since the last start.
- ISOs are attached only at create; an ejected drive can't be refilled.
- Create passes FluxVM `tpm` (`vtpm`), `secure_boot`, optional `fluxvm_firmware` (OVMF path) and `fluxvm_balloon`.
  When `fluxvm_firmware` is empty, UEFI still comes from the host-wide `qemu_ovmf_code` in `/etc/fluxvm.toml`.
- CD-ROM insert and live-mig after hotplug / with extra NICs remain FluxVM limits (not Machina gaps). Cross-host
  migrate between different QEMU versions still needs FluxVM to pin the source machine type on the receiver.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| `fluxvm/status` says unreachable | `systemctl status fluxvm`, `base_url`, token |
| No FluxVM VMs in the platform inventory | `MACHINA_FLUXVM_URL` on the agent, then `systemctl restart machina-agent` |
| Migration pre-check fails `mobility` | Engine isn't QEMU, disk isn't shared, the VM has extra NICs or an ISO in a drive, or it was hot-added or had its last NIC removed since it started (restart it) |
| Migration fails with `eject cdrom "<name>" first` | Eject the ISO: **Manage → Install media → Eject**, or `POST /api/v1/vms/{name}/cdrom/eject/{drive}?backend=fluxvm`. Works on a running VM |
| Create fails naming `cdrom "install"` and `allowed_image_dirs` | Move the ISO under one of FluxVM's `policy.allowed_image_dirs` (often `/var/lib/fluxvm/images`) |
| Create fails with `fluxvm_isos need fluxvm_backend qemu` | Only QEMU has CD-ROMs; pick `qemu` or `auto` |
| The VM boots the old OS instead of the installer | The root disk isn't blank; firmware boots it before the CD-ROM. Use an empty file |
| Windows Setup finds no disk | Load `viostor` from the virtio-win ISO (`cd2`) |
| Serial tab shows nothing during a Windows install | Windows doesn't use the serial port; open **Console (VNC)** (ISO install step 3) |
| HA re-create fails on a missing ISO | The VM still had media in a drive and the ISO isn't on the target host; eject it, or put the ISO at the same path there |
| Create fails with `fluxvm-api POST /v1/vms unreachable` after ~5 min | Disk provisioning outran the daemon's 300 s request timeout (FluxVM then reaps the VM stuck in `Creating`). Check host disk load (`/proc/pressure/io`) |
| Create or start fails on a VM-edge attach error after a FluxVM upgrade | FluxVM now requires its eBPF edge by default; install the BPF objects or set `[sandbox.dataplane] mode = "legacy"` (see [Upgrading FluxVM](#upgrading-fluxvm)) |
| NIC removal fails with `guest did not release nicN` | The guest isn't booted far enough to handle PCIe unplug; retry once it's up |
| HA re-create fails on the disk lock | Another instance still runs on the disk; stop it, or check fencing |
| Serial shows nothing on a non-QEMU VM | It's the read-only log; use the Agent console for input |
