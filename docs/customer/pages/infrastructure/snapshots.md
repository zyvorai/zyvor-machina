# Snapshots

## Purpose

Snapshots — Machina Infrastructure page at `/snapshots`.

## When to use it

- Operate **Snapshots** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/snapshots`
- Nav: **Infrastructure → Snapshots** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Per-VM**.
3. Create.
4. Revert.
5. Delete.
6. Refresh.
7. **Empty:** No snapshots.
8. **Success:** Snapshot listed; revert OK.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Virtual Machines](../core/vms.md)
- [Fleet snapshot schedules](../platform/platform-fleet-snapshots.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
