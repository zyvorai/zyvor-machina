# Platform events

## Purpose

Event Log — Machina Platform page at `/platform/events`.

## When to use it

- Operate **Platform events** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/events`
- Nav: **Platform → Event Log** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **All / Audit / Events / Tasks**.
3. Apply filters.
4. Refresh.
5. **Empty:** No platform events.
6. **Success:** Filtered rows appear.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Audit Log](../monitoring/audit.md)
- [Tasks](platform-tasks.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
