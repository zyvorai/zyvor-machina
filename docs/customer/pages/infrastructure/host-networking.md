# Host Networking

## Purpose

Host Networking — Machina Infrastructure page at `/host-networking`.

## When to use it

- Operate **Host Networking** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/host-networking`
- Nav: **Infrastructure → Host Networking** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Bridges/routes/DNS/firewall**.
3. Create Bridge.
4. Add Rule.
5. Refresh diagnostics.
6. **Empty:** —.
7. **Success:** Bridge/rule applied.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Networks](networks.md)
- [Host overview](../monitoring/node.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
