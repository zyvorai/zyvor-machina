# Leaving Nutanix for Machina

Transiva exports a VM out of Nutanix through the Prism v4 API. h2kvm converts it and can emit a libvirt domain definition (`--emit-domain-xml`) for a Machina libvirt host.

> **Status: Machina itself was not used in the Nutanix test.** What was run end to end is Nutanix export, h2kvm conversion, and a boot under plain libvirt/KVM and on KubeVirt, against Chimera's Nutanix simulator. Machina's libvirt host path is the documented h2kvm target, not something verified with a Nutanix source.

## The path

```text
Nutanix (Prism v4)  ->  hyperexport  ->  h2kvm  ->  libvirt domain on a Machina host
```

1. **Export** with Transiva: `hyperexport --provider nutanix -vm <name> -resolve-containers -mounts <container>:<nfs-path>`.
2. **Convert** with h2kvm and `--emit-domain-xml` to get the disk and a domain definition.
3. **Land** on a Machina libvirt host. libvirt accepted the domain XML h2kvm emitted (UEFI/OVMF, virtio NIC, VNC and SPICE graphics) without edits; an older h2kvm build had emitted an invalid `graphics type='both'`, so use a current h2kvm.

## Measured elsewhere in the path

Export, convert and a libvirt/KVM boot (UEFI, from the domain XML h2kvm emitted) passed in an unattended run on an Ubuntu 24.04 cloud image: 7 of 7 stages, 747 s; the guest answered ping 79 s after boot started ([case study](https://github.com/zyvorai/zyvor-transiva/blob/main/docs/case-study-nutanix-exit-linux.md)).

## Not covered

- Booting the converted VM through Machina.
- A real Nutanix cluster, Windows, UEFI, multi-disk or large VMs.
- Boot repair on a guest that needs it (the Ubuntu test image needed only h2kvm's DHCP network fallback).

Related: [kubevirt-migration.md](../kubevirt-migration.md), [INSTALL.md](../INSTALL.md).
