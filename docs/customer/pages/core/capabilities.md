# Capabilities

## Purpose

Capabilities — Machina Core page at `/capabilities`.

## When to use it

- Operate **Capabilities** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/capabilities`
- Nav: **Core → Capabilities** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Capabilities / System Info**.
3. Refresh.
4. **Empty:** —.
5. **Success:** Arch/features listed.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Host overview](../monitoring/node.md)
- [Node Devices](devices.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
