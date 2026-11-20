# Backups (Snapshot Backups)

## Purpose

Snapshot Backups — Machina Infrastructure page at `/backups`.

## When to use it

- Operate **Backups (Snapshot Backups)** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/backups`
- Nav: **Infrastructure → Snapshot Backups** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. New Backup.
3. Scheduled Backup.
4. Verify checksums.
5. **Empty:** No backups.
6. **Success:** Backup completed/verified.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Time Machine](../platform/platform-backups.md)
- [Snapshots](snapshots.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
