# Alerts

## Purpose

Alerts — Machina Platform page at `/platform/notifications`.

## When to use it

- Operate **Alerts** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/notifications`
- Nav: **Platform → Alerts** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Open runbook/action.
3. Acknowledge.
4. **Empty:** No alerts — you are caught up.
5. **Success:** Alert cleared.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Security Operations Center](platform-soc.md)
- [Platform events](platform-events.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
