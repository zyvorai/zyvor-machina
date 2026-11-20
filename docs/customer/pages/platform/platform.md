# Mission Control

## Purpose

Mission Control — multi-host platform overview.

## When to use it

- Operate **Mission Control** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform`
- Nav: **Platform → Mission Control** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **?mission=1** geography; host/VM sidebar rail.
3. Create VM (wizard).
4. Use Launchpad tiles.
5. Power VM.
6. Ask Zyra.
7. Geography expand.
8. Live Wall.
9. **Empty:** No hosts → Add host / enroll.
10. **Success:** Hosts online; VM create task; hero Operational.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Machine Finder](platform-vms.md)
- [Hosts](platform-hosts.md)
- [Time Machine](platform-backups.md)
- [Host Enrollment](platform-enroll.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
