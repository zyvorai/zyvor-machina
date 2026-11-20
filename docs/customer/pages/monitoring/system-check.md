# System Check

## Purpose

System Check — Machina Monitoring page at `/system-check`.

## When to use it

- Operate **System Check** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/system-check`
- Nav: **Monitoring → System Check** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Check categories**.
3. Run again.
4. **Empty:** —.
5. **Success:** All checks pass / actionable fails listed.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Dashboard](../core/home.md)
- [Host overview](node.md)
- [Capabilities](../core/capabilities.md)
- [Getting Started](../../getting-started.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
