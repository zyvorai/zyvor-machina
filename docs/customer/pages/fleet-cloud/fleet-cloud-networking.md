# Networking

## Purpose

Networking — Machina Fleet Cloud page at `/fleet-cloud/networking`.

## When to use it

- Operate **Networking** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/networking`
- Nav: **Fleet Cloud → Networking** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Networks/ports**.
3. Create network.
4. Create port.
5. Delete.
6. Refresh.
7. **Empty:** —.
8. **Success:** Network/port created.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Network topology](fleet-cloud-topology.md)
- [Security Groups](fleet-cloud-security-groups.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
