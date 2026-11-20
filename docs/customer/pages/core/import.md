# Import guest VM

## Purpose

Import VM — Machina Core page at `/import`.

## When to use it

- Operate **Import guest VM** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/import`
- Nav: **Core → Import VM** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Source path picker**.
3. Browse disk.
4. Import from Fleet Cloud Images.
5. Create VM.
6. **Empty:** Need readable disk image.
7. **Success:** Domain defined; appears on /vms.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Virtual Machines](vms.md)
- [Disk Images](../infrastructure/disk-images.md)
- [Images](../fleet-cloud/fleet-cloud-images.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
