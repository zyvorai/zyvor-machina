# Network canvas

## Purpose

Network Canvas — Machina Platform page at `/platform/network-canvas`.

## When to use it

- Operate **Network canvas** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/network-canvas`
- Nav: **Platform → Network Canvas** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Canvas interactions**.
3. Inspect paths.
4. Open Security.
5. **Empty:** —.
6. **Success:** Path/trace visible.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Networks](platform-networks.md)
- [Security Center](../platform-security/platform-zeus-security.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
