# Tutorial: FluxVM day two

Run a QEMU VM on [FluxVM](https://github.com/zyvorai/zyvor-fluxvm) through Machina the same way you run a libvirt guest:
browser VNC, a data disk, a snapshot, a backup and a clone. About 20 minutes on a lab host. Everything is prefixed
`tut-` so you can delete it at the end.

**You need:** Machina daemon on `:5092` with `[fluxvm] enabled = true`, FluxVM **0.4.18+** (`fluxctl --version`) serving
`http://127.0.0.1:7788`, a raw guest image and a kernel+initrd on the host (the regression defaults work:
`/var/lib/fluxvm/images/linux-agent.raw`, `/var/lib/fluxvm/kernels/bionic-vmlinuz-4.15`,
`/var/lib/fluxvm/kernels/bionic-initrd-4.15`). Full matrix and fleet migrate/HA: [FluxVM guide](../fluxvm.md).

## 0. Shell helpers
```bash
export HOST=https://127.0.0.1:5092
export API=$HOST/api/v1
# PAM login → session cookie (self-signed TLS is fine with -k)
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
mc "$API/vms/tut-flux?backend=fluxvm"   # state: running, backend: fluxvm
```
In the UI: **Create VM → FluxVM**, same fields. The VM also appears in the ordinary VM list.

## 3. Open the graphical console
Open **VM detail → Console (VNC)**. Machina mints a FluxVM `ws-ticket` and proxies RFB — no `socat` on the host.

From a shell you only need to prove the socket answers (RFB greeting starts with `RFB `); use the UI for real work.

## 4. Attach, grow and detach a data disk
```bash
mc -X POST "$API/vms/tut-flux/disk/attach?backend=fluxvm" \
  -d '{"target":"data1","source":"","size_gb":1}'
mc -X POST "$API/vms/tut-flux/disk/resize/data1?backend=fluxvm" -d '{"size_gb":2}'
mc "$API/vms/tut-flux?backend=fluxvm"    # disks include data1 at 2 GiB
mc -X POST "$API/vms/tut-flux/disk/detach/data1?backend=fluxvm"
```
Same path in **Manage → Disks**.

## 5. Snapshot, then stop for clone
```bash
mc -X POST "$API/vms/tut-flux/snapshots?backend=fluxvm" -d '{"name":"tut-s1"}'
mc "$API/vms/tut-flux/snapshots?backend=fluxvm"
mc -X POST "$API/vms/tut-flux/stop?backend=fluxvm"
# wait until state is shutoff
mc -X POST "$API/vms/tut-flux/clone?backend=fluxvm" -d '{"new_name":"tut-flux-clone"}'
mc -X POST "$API/vms/tut-flux-clone/rename?backend=fluxvm" -d '{"new_name":"tut-flux-copy"}'
```

## 6. Backup and restore (stopped guest)
```bash
mc -X POST $API/backups -d '{"vm_name":"tut-flux","backend":"fluxvm","compress":false}'
mc "$API/backups?backend=fluxvm&vm=tut-flux"
mc -X POST $API/backups/restore -d '{"backup_id":"<id from list>","backend":"fluxvm","vm_name":"tut-flux"}'
mc -X POST "$API/vms/tut-flux/start?backend=fluxvm"
```

## You should see
`tut-flux` running on FluxVM, VNC in the browser, a grown-then-detached data disk, a snapshot, a renamed clone, and a
restored disk that boots again. `GET /api/v1/vms` lists it next to any libvirt guests (`backend: "fluxvm"`).

## Clean up
```bash
mc -X DELETE "$API/vms/tut-flux?backend=fluxvm"
mc -X DELETE "$API/vms/tut-flux-copy?backend=fluxvm"
# delete any tut-* backups: DELETE $API/backups/<id>?backend=fluxvm
```

## Next
- Shared-disk live migrate and fleet HA re-create: [FluxVM guide](../fluxvm.md) and `make regression-fluxvm`.
- Site docs: [FluxVM backend](https://zyvorai.github.io/zyvor-machina/docs/core-concepts/fluxvm).
- Previous tutorial: [Automate it](06-api-and-terraform.md).
