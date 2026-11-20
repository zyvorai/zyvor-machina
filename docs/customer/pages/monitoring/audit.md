# Audit Log

## Purpose

Audit Log — Machina Monitoring page at `/audit`.

## When to use it

- Operate **Audit Log** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/audit`
- Nav: **Monitoring → Audit Log** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Refresh now.
3. **Empty:** —.
4. **Success:** Audit rows after actions.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Platform events](../platform/platform-events.md)
- [Web sessions](admin-sessions.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
