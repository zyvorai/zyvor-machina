# Placement & HA

## Purpose

Disaster Recovery — Machina Platform page at `/platform/placement`.

## When to use it

- Operate **Placement & HA** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/placement`
- Nav: **Platform → Disaster Recovery** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Recs / migrations / fence / HA events**.
3. Toggle DRS auto-migrate.
4. Refresh.
5. Migrate (precheck).
6. **Empty:** Cluster load balanced (no recs).
7. **Success:** Migration job recorded.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [HA Events](platform-ha.md)
- [Migration Radar](platform-migration.md)
- [Machine Finder](platform-vms.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
