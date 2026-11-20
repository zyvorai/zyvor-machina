# Observability

## Purpose

Observability — Machina Platform page at `/platform/observability`.

## When to use it

- Operate **Observability** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/observability`
- Nav: **Platform → Observability** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **SLO / traces panels**.
3. Refresh.
4. **Empty:** No SLO policies / No API traces.
5. **Success:** Metrics & traces present.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Activity Monitor](platform-activity.md)
- [Live Metrics](../monitoring/events.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
