# Tutorial: bring your own disks

Move a VM from another hypervisor by importing its disk: convert it to qcow2, define a VM around it, boot it. This covers the
disk, not the whole machine: networking, drivers and licences are yours to check. About 20 minutes per VM.

**Verified live:** VMDK, VDI and raw disks imported, VMs created from them booted, got a DHCP address and answered ping
([claims ledger](../claims.md), C23). VHD and `.img` were broken until PR #72 (wrong `qemu-img` format names); use a build that
includes it. Larger migrations (KubeVirt) are in [kubevirt-migration](../kubevirt-migration.md).

## You need
A disk file on the Machina host (copy it there with `scp` or a shared mount), `qemu-img` installed (it ships with QEMU), and an
admin or operator token. The import reads an absolute path off the host's filesystem, so it needs the permission to browse host
paths.

## 1. Export from the source
Export the VM's disk from VMware (VMDK), VirtualBox (VDI), Hyper-V (VHD) or any raw image. Shut the source VM down first so the
disk is consistent. Copy it to the host, for example `/var/tmp/app1.vmdk`.

## 2. Import it as qcow2
In the UI: **Import** (Classic, `/import`), or the API on the daemon:
```bash
curl -sk -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -X POST https://HOST:5092/api/v1/import/disk -d '{"source":"/var/tmp/app1.vmdk","dest_name":"app1"}'
# {"status":"imported","path":"/var/lib/libvirt/images/app1.qcow2"}
```
Accepted: `.qcow2` (copied), `.vmdk`, `.vdi`, `.raw`, `.img`, `.vhd`/`.vpc`. Refused with a clear error: a relative path, a missing
file, an unsupported extension, a destination name that tries to escape the images directory, and a destination that already exists.

## 3. Create a VM around the disk
```bash
curl -sk -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' -X POST https://HOST:5092/api/v1/vms \
  -d '{"name":"app1","vcpus":2,"memory_mb":2048,"disk_gb":0,"existing_disk":"/var/lib/libvirt/images/app1.qcow2","network":"default"}'
```
`disk_gb: 0` with `existing_disk` means "use this disk as is". The UI does the same in its second step.

## 4. Boot and check
Start it, open the console, and confirm the guest comes up. A Linux guest with DHCP gets an address on the `default` network.

## You should see
A qcow2 file of the same virtual size as the source, a VM that boots from it, and (for DHCP guests) an address you can ping.

## Clean up
Delete the VM, then remove the disk file and the source copy.

## Know the edges
- Windows guests usually need virtio drivers (or an emulated disk and NIC first); the import does not inject drivers.
- Guests with static addresses or MAC-bound licences need attention by hand.
- Only the disk moves: no VM definition, snapshots or networks from the source.
- For many VMs, script the two calls above; there is no bulk importer today.
