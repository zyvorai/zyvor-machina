# Storage (Disk Utility)

## Purpose

Disk Utility — Machina Platform page at `/platform/storage`.

## When to use it

- Operate **Storage (Disk Utility)** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/storage`
- Nav: **Platform → Disk Utility** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Disks / Pools / Tiers / Backup SLA**.
3. Sync hosts.
4. Add pool.
5. New volume.
6. Delete.
7. **Empty:** No storage pools → import/add.
8. **Success:** Pool active; volume created.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Storage pools](../infrastructure/storage.md)
- [Storage (Atlas)](platform-storage-atlas.md)
- [Storage Tiers](platform-storage-tiers.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
