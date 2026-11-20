# Migration Assistant

## Purpose

Migration Radar: bring VMs into Machina from OVF/OVA, VMDK or cloud images. GuestKit checks disks offline before and after conversion.

## When to use it

- Import a single OVF/OVA, VMDK or cloud image
- Follow GuestKit offline inspection and doctor jobs

## How to get there

- Route: `/platform/migration`
- Nav: **Platform → Migration Assistant** (or spotlight / Finder search)

## What you can do

1. Pick a source: **OVF / OVA File**, **VMDK File** or **Cloud Image** — each opens the import wizard.
2. **Plan waves** groups VMs into migration waves (needs GuestKit).
3. **GuestKit jobs** (`?tab=jobs`) is the queue of offline inspect / doctor / migrate-plan jobs; enter a disk path such as `/var/lib/libvirt/images/vm.qcow2` to run one.

## If something is wrong

- **GuestKit tabs disabled:** GuestKit is not enabled (`GUESTKIT_ENABLED` on the controller, `[guestkit]` on the daemon).

## Related pages

- [Import guest VM](../core/import.md)
- [Placement & HA](platform-placement.md)
- [Machine Finder](platform-vms.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
