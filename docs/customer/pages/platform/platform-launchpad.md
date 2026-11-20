# Launchpad (on Mission Control)

## Purpose

Launchpad tiles live on Mission Control (`/platform`) — there is no separate `/platform/launchpad` route.

## When to use it

- Operate **Launchpad (on Mission Control)** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/launchpad`
- Nav: **Platform → Launchpad (Mission Control)** (or spotlight / Finder search)

## Notes

- There is **no** standalone `/platform/launchpad` route — Launchpad tiles are embedded on Mission Control.

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Operations launchpad on Mission Control**.
3. Open /platform.
4. Use Operations launchpad tiles (Create VM, Security, Finder, Recovery, GPU, Migration, Templates, Cinema, Live Wall).
5. **Empty:** There is no separate /platform/launchpad route.
6. **Success:** Tile opens the target surface.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Mission Control](platform.md)
- [Blueprint Studio](platform-blueprints.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Page index](../../PAGE_INDEX.md)
