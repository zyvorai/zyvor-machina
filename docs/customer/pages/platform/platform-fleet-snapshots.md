# Fleet snapshot schedules

## Purpose

Fleet Snapshots — Machina Platform page at `/platform/fleet-snapshots`.

## When to use it

- Operate **Fleet snapshot schedules** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/fleet-snapshots`
- Nav: **Platform → Fleet Snapshots** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Project/tag filters**.
3. Add schedule.
4. Delete.
5. **Empty:** No fleet schedules yet.
6. **Success:** Schedule listed/enabled.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Snapshots](../infrastructure/snapshots.md)
- [Time Machine](platform-backups.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
