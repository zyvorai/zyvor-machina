---
sidebar_position: 7
title: FluxVM day two
description: Create a FluxVM QEMU guest through Machina, open VNC, attach a disk, snapshot, backup and clone.
---

# FluxVM day two

Run a QEMU VM on [FluxVM](https://github.com/zyvorai/zyvor-fluxvm) through Machina the same way you run a libvirt guest:
browser VNC, a data disk, a snapshot, a backup and a clone. About 20 minutes on a lab host. Everything is prefixed
`tut-` so you can delete it at the end.

**You need:** Machina daemon on `:5092` with `[fluxvm] enabled = true`, FluxVM **0.4.18+** (`fluxctl --version`) serving
`http://127.0.0.1:7788`, a raw guest image and a kernel+initrd on the host (defaults used by the live regression:
`/var/lib/fluxvm/images/linux-agent.raw`, `/var/lib/fluxvm/kernels/bionic-vmlinuz-4.15`,
`/var/lib/fluxvm/kernels/bionic-initrd-4.15`). Full matrix and fleet migrate/HA: [FluxVM backend](../core-concepts/fluxvm.md)
and the [operator guide](https://github.com/zyvorai/zyvor-machina/blob/main/docs/fluxvm.md).

## 0. Shell helpers

```bash
export HOST=https://127.0.0.1:5092
export API=$HOST/api/v1
curl -sk -c /tmp/mc.cj -b /tmp/mc.cj -X POST $API/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"YOUR_USER","password":"YOUR_PASS"}'
alias mc='curl -sk -b /tmp/mc.cj -c /tmp/mc.cj -H "Content-Type: application/json"'
```

## 1. Confirm FluxVM is wired in

```bash
mc $API/fluxvm/status
# enabled: true, reachable: true
```

If `reachable` is false, start `fluxvm.service` and check `base_url` in `/etc/machina/config.toml`. Browser VNC needs a
FluxVM build that exposes `POST /v1/vms/{id}/ws-ticket` (0.4.18+).

## 2. Create a small QEMU guest

```bash
mc -X POST $API/vms -d '{
  "name": "tut-flux",
  "vcpus": 1,
  "memory_mb": 1024,
  "disk_gb": 0,
  "backend": "fluxvm",
  "fluxvm_backend": "qemu",
  "fluxvm_image": "/var/lib/fluxvm/images/linux-agent.raw",
  "fluxvm_kernel": "/var/lib/fluxvm/kernels/bionic-vmlinuz-4.15",
  "fluxvm_initrd": "/var/lib/fluxvm/kernels/bionic-initrd-4.15",
  "fluxvm_kernel_args": "console=ttyS0 root=/dev/vda rw",
  "fluxvm_bridge": "virbr0"
}'
mc "$API/vms/tut-flux?backend=fluxvm"
```

In the UI: **Create VM → FluxVM**. The guest appears in the ordinary VM list with `backend: "fluxvm"`.

## 3. Open the graphical console

Open **VM detail → Console (VNC)**. Machina mints a FluxVM `ws-ticket` and proxies RFB — no host `socat`.

## 4. Attach, grow and detach a data disk

```bash
mc -X POST "$API/vms/tut-flux/disk/attach?backend=fluxvm" \
  -d '{"target":"data1","source":"","size_gb":1}'
mc -X POST "$API/vms/tut-flux/disk/resize/data1?backend=fluxvm" -d '{"size_gb":2}'
mc -X POST "$API/vms/tut-flux/disk/detach/data1?backend=fluxvm"
```

Same path in **Manage → Disks**.

## 5. Snapshot, then stop for clone

```bash
mc -X POST "$API/vms/tut-flux/snapshots?backend=fluxvm" -d '{"name":"tut-s1"}'
mc -X POST "$API/vms/tut-flux/stop?backend=fluxvm"
# wait until shutoff
mc -X POST "$API/vms/tut-flux/clone?backend=fluxvm" -d '{"new_name":"tut-flux-clone"}'
mc -X POST "$API/vms/tut-flux-clone/rename?backend=fluxvm" -d '{"new_name":"tut-flux-copy"}'
```

## 6. Backup and restore (stopped guest)

```bash
mc -X POST $API/backups -d '{"vm_name":"tut-flux","backend":"fluxvm","compress":false}'
mc "$API/backups?backend=fluxvm&vm=tut-flux"
mc -X POST $API/backups/restore -d '{"backup_id":"<id>","backend":"fluxvm","vm_name":"tut-flux"}'
mc -X POST "$API/vms/tut-flux/start?backend=fluxvm"
```

## You should see

`tut-flux` running on FluxVM, VNC in the browser, a grown-then-detached data disk, a snapshot, a renamed clone, and a
restored disk that boots again.

## Clean up

```bash
mc -X DELETE "$API/vms/tut-flux?backend=fluxvm"
mc -X DELETE "$API/vms/tut-flux-copy?backend=fluxvm"
```

## Next

- Shared-disk live migrate and fleet HA: [FluxVM backend](../core-concepts/fluxvm.md).
- Hands-on path in the repo: [tutorial 07](https://github.com/zyvorai/zyvor-machina/blob/main/docs/tutorials/07-fluxvm-day-two.md).
- [Quickstart](./quickstart.md) if you are still installing.
