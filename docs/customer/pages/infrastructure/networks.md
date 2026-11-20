# Networks

## Purpose

Networks — Machina Infrastructure page at `/networks`.

## When to use it

- Operate **Networks** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/networks`
- Nav: **Infrastructure → Networks** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Create network.
3. Start/Stop.
4. Edit XML.
5. **Empty:** No libvirt networks.
6. **Success:** Network active.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Networks](../platform/platform-networks.md)
- [Network Filters](nwfilters.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
