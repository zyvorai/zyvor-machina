# Leaving Nutanix for Machina

Transiva exports a VM out of Nutanix through the Prism v4 API. h2kvm converts it and can emit a libvirt domain definition (`--emit-domain-xml`) for a Machina libvirt host.

> **Status: Machina itself was not used in the Nutanix test.** What was run end to end is Nutanix export, h2kvm conversion, and a boot under plain libvirt/KVM and on KubeVirt, against Chimera's Nutanix simulator. Machina's libvirt host path is the documented h2kvm target, not something verified with a Nutanix source.

## The path

```text
Nutanix (Prism v4)  ->  hyperexport  ->  h2kvm  ->  libvirt domain on a Machina host
```

1. **Export** with Transiva: `hyperexport --provider nutanix -vm <name> -resolve-containers -mounts <container>:<nfs-path>`.
2. **Convert** with h2kvm and `--emit-domain-xml` to get the disk and a domain definition.
3. **Land** on a Machina libvirt host. Review the XML first: in the test h2kvm wrote `graphics type='both'`, which libvirt rejected, and it had to be changed to `vnc` by hand.

## Measured elsewhere in the path

Export, convert and a libvirt/KVM boot passed in an unattended run: 7 of 7 stages, 145 s; the guest answered ping 35 s after boot started ([case study](https://github.com/zyvorai/zyvor-transiva/blob/main/docs/case-study-nutanix-exit-linux.md)).

## Not covered

- Booting the converted VM through Machina.
- A real Nutanix cluster, Windows, UEFI, multi-disk or large VMs.
- Guest repair (skipped on the test image).

Related: [kubevirt-migration.md](../kubevirt-migration.md), [INSTALL.md](../INSTALL.md).
