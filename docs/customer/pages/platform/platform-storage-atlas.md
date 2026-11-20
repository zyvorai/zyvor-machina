# Storage (Atlas)

## Purpose

Storage (Atlas) — Machina Platform page at `/platform/storage-atlas`.

## When to use it

- Operate **Storage (Atlas)** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/storage-atlas`
- Nav: **Platform → Storage (Atlas)** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Backend cards (Ceph/NFS/ZFS)**.
3. New volume.
4. Restore.
5. **Empty:** Atlas disabled / unreachable.
6. **Success:** Backend healthy; volume ops.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Storage (Disk Utility)](platform-storage.md)
- [Time Machine](platform-backups.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
