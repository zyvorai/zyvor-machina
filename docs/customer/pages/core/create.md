# Create new guest VM

## Purpose

Create a new libvirt VM.

## When to use it

- Operate **Create new guest VM** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/create`
- Nav: **Core → Create VM** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Modes: install vs golden; Source & OS → Disk → Network → Cloud-init → Review**.
3. Install from media or Clone golden image.
4. Browse ISO.
5. Create VM.
6. Save defaults.
7. **Empty:** Need pool/volume or new disk.
8. **Success:** Job/console opens; guest listed on /vms.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Virtual Machines](vms.md)
- [Disk Images](../infrastructure/disk-images.md)
- [Storage pools](../infrastructure/storage.md)
- [Create VM from ISO](../platform/platform-create-iso.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
