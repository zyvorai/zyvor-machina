# Blueprint Studio

## Purpose

Blueprints — Machina Platform page at `/platform/blueprints`.

## When to use it

- Operate **Blueprint Studio** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/blueprints`
- Nav: **Platform → Blueprints** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Launchpad / Studio**.
3. New shortcut.
4. Create from VMs.
5. AI generate.
6. Run.
7. Delete.
8. **Empty:** Create shortcut to populate Launchpad.
9. **Success:** Blueprint run → tasks.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Tasks](platform-tasks.md)
- [Machine Finder](platform-vms.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
