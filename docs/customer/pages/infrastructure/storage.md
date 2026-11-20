# Storage pools

## Purpose

Storage Pools — Machina Infrastructure page at `/storage`.

## When to use it

- Operate **Storage pools** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/storage`
- Nav: **Infrastructure → Storage Pools** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Pool → volumes**.
3. Create volume.
4. Clone.
5. Resize.
6. Refresh.
7. **Empty:** No storage pools / No volumes.
8. **Success:** Volume listed.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Storage (Disk Utility)](../platform/platform-storage.md)
- [Disk Images](disk-images.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
