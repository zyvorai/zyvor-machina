---
sidebar_position: 5
title: Migrating VMs in
description: Move VMs from VMware, Hyper-V, Nutanix, Proxmox and public clouds into Machina with Transiva, h2kvm and GuestKit.
---

# Migrating VMs in

Machina does not convert disks itself. Three sibling Zyvor tools do that, and Machina imports the result as an ordinary libvirt
domain, so HA, DRS, live migration and snapshots apply from the first boot.

| Tool | Job |
|---|---|
| **Transiva** | Discovers VMs on the source and runs the pipeline: export, convert, deploy. Schedules and tracks jobs. |
| **h2kvm** | Converts VMDK, VHDX, OVA/OVF and raw disks to KVM and fixes the guest offline. |
| **GuestKit** | Inspects the guest offline, repairs fstab, bootloader and initramfs, and scores boot readiness. |

## Sources Transiva reads

vSphere, Hyper-V, Nutanix, Proxmox, AWS, Azure, GCP, OCI, OpenStack and Alibaba. h2kvm also accepts a disk file directly
(VMDK, VHDX, OVA/OVF, raw) when there is no live source.

## The path

1. **Connect the source.** In Transiva set the source credentials (for example `GOVC_URL`, `GOVC_USERNAME`, `GOVC_PASSWORD` for
   vSphere, or `HYPERV_HOST` for Hyper-V) and run `transivactl list -provider vsphere`.
2. **Submit the job.** Export, convert with h2kvm, repair the guest, then define it on the target:
   `transivactl job submit -provider vsphere -vm /datacenter/vm/web-01 --metadata '{"migrate":true,"deploy_target":"libvirt","vm_name":"web-01"}'`.
3. **Or import in Machina.** Use **Import VM** in the web UI, or have h2kvm emit libvirt domain XML (`--emit-domain-xml`) and define
   it on the host.
4. **Check and start.** Boot the VM, confirm the console and network, then enrol it in an HA policy if it should fail over.

Windows guests take the VirtIO and registry path automatically; a VM whose name contains `win` selects Windows defaults (SATA bus,
e1000e NIC, UEFI). After deploy, use the RDP console from the VM page.

## Moving many VMs

Batch exports (`transiva-export -batch vms.txt`) and Transiva's scheduler cover waves. Machina does not yet plan waves or track
cutover per application inside the product; keep the wave list in Transiva or your ticketing system.

## What to know

- Transiva and h2kvm are separate products with their own licences; see the [claims ledger](https://github.com/zyvorai/zyvor-machina/blob/main/docs/claims.md) (C54 for the Machina-side Transiva API, C56 for scope) for what is verified in this repository.
- Machina has no live replication from the source: a VM is down or quiesced during final sync and cutover.
- Machina can also take KubeVirt VMs the other way; see the KubeVirt migration guide in the repository docs.

Related: [Replacing OpenStack](./from-openstack.md) and the [Machina vs VMware](/vs-vmware) comparison.
