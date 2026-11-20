# Volume Snapshots

## Purpose

Volume Snapshots — Machina Fleet Cloud page at `/fleet-cloud/volume-snapshots`.

## When to use it

- Operate **Volume Snapshots** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/volume-snapshots`
- Nav: **Fleet Cloud → Volume Snapshots** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Create snapshot.
3. Delete.
4. Refresh.
5. **Empty:** No snapshots.
6. **Success:** Snapshot listed.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Volumes](fleet-cloud-volumes.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
