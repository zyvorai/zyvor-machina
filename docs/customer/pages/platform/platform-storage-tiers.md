# Storage Tiers

## Purpose

Storage Tiers — Machina Platform page at `/platform/storage-tiers`.

## When to use it

- Operate **Storage Tiers** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/storage-tiers`
- Nav: **Platform → Storage Tiers** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Define/edit tiers.
3. **Empty:** No storage tiers.
4. **Success:** Tier list non-empty.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Storage (Disk Utility)](platform-storage.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
