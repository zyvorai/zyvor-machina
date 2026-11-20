# Node Devices

## Purpose

Node Devices — Machina Core page at `/devices`.

## When to use it

- Operate **Node Devices** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/devices`
- Nav: **Core → Node Devices** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Refresh.
3. **Empty:** No devices found.
4. **Success:** PCI/USB tree visible.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Host overview](../monitoring/node.md)
- [Capabilities](capabilities.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
