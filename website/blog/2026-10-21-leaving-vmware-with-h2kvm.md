---
slug: leaving-vmware-with-h2kvm
title: Leaving VMware without moving to Azure
authors: [zyvor]
tags: [vmware, migration, h2kvm]
description: How h2kvm and Machina take a VM from vSphere to KVM with the guest fixed before first boot.
---

Most VMware exits fail after cutover: the bootloader is wrong, or Windows still points at the old hypervisor. h2kvm repairs the guest offline first, then converts it.

<!-- truncate -->

1. **Pick up** from vSphere, ESXi, Azure or a disk file (VMDK, VHDX, OVA/OVF, raw).
2. **Repair** with GuestKit: fstab, bootloader, initramfs, Windows VirtIO drivers.
3. **Convert** to a KVM-ready disk.
4. **Land** on Machina through Import VM, where HA, DRS and live migration take over.

Compare the whole platform on the [Machina vs VMware](/vs-vmware) page.
