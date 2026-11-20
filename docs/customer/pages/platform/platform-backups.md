# Backup & Restore

## Purpose

Fleet backups: a timeline of backup runs, backup destinations (NFS, S3/MinIO or local), recurring schedules by project or tag, and one-off backups of a single VM.

## When to use it

- Register where backups are stored
- Back up every VM in a project or with a tag on a schedule, with retention
- Queue an immediate full or incremental backup of one VM
- Restore a VM

## How to get there

- Route: `/platform/backups`
- Nav: **Platform → Backup & Restore** (or spotlight / Finder search)

## What you can do

1. **Backup destinations**: register an NFS, S3 or local target and pick the default target.
2. **Queue VM backup**: enter a VM id, choose **Full backup** (whole qcow2) or **Incremental** (chains on the previous completed backup on that host), then **Queue backup**. The job shows up in [Tasks](platform-tasks.md).
3. **New backup schedule**: name, interval in hours, retain count, and an optional project or tag filter. **Active schedules** lists them; delete with confirmation.
4. **Restore VM from backup**: restore runs from the VM detail page, where each VM's backup history lives (**Browse VMs**).

## If something is wrong

- **Incremental fails:** there is no completed full backup for that VM on its current host yet; run a full one first.
- **S3 target errors:** check endpoint, bucket and credentials, and that the hosts can reach the endpoint.
- Snapshot schedules (not backups) live in [Fleet snapshot schedules](platform-fleet-snapshots.md).

## Related pages

- [Backups (Snapshot Backups)](../infrastructure/backups.md)
- [Machine Finder](platform-vms.md)
- [Fleet snapshot schedules](platform-fleet-snapshots.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
