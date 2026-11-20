# Support Assistant

## Purpose

Support — Machina Platform page at `/platform/support`.

## When to use it

- Operate **Support Assistant** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/support`
- Nav: **Platform → Support** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Export support bundle.
3. **Empty:** —.
4. **Success:** Bundle downloads.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Platform events](platform-events.md)
- [System Logs](../monitoring/logs.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
