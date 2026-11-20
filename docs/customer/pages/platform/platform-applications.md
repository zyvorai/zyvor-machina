# Applications

## Purpose

Applications — Machina Platform page at `/platform/applications`.

## When to use it

- Operate **Applications** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/applications`
- Nav: **Platform → Applications** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **App Launchpad icons**.
3. New Application.
4. Start all.
5. Stop all.
6. Backup all VMs.
7. **Empty:** No application groups → Create Application (need VMs).
8. **Success:** Group starts/stops as stack.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Machine Finder](platform-vms.md)
- [Time Machine](platform-backups.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
