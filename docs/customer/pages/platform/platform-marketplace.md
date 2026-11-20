# Marketplace

## Purpose

Marketplace — Machina Platform page at `/platform/marketplace`.

## When to use it

- Operate **Marketplace** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/marketplace`
- Nav: **Platform → Marketplace** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Featured**.
3. Install / Installed plugins.
4. **Empty:** No plugins available.
5. **Success:** Plugin Installed.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Marketplace (Templates)](platform-templates.md)
- [Applications](platform-applications.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
